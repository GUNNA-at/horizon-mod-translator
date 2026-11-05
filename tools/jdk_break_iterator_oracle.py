#!/usr/bin/env python3
"""JDK oracle for horizon-compat's `java.text.BreakIterator` (crates/horizon-compat/src/breaks.rs).

Reads the cases of the test `text_breaks_where_the_jdks_iterators_break_it`, runs each text through
the JDK's own iterators (the root locale's) and prints the rows as the JDK breaks them, in the test's
form. With --diff it prints only the rows where the test expects something else. Use the Java the
game runs (21 for Minecraft 1.21.1): characters break by the Unicode version of the JDK.

    python3 tools/jdk_break_iterator_oracle.py [--java-home /path/to/jdk-21] [--diff]
"""

import argparse
import os
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEST = ROOT / "crates/horizon-compat/src/breaks.rs"
ROW = re.compile(r'^\s*\("(\w+)", "((?:[^"\\]|\\.)*)", &\[([0-9, ]*)\]\),\s*$')

ORACLE = r"""
import java.io.*;
import java.text.BreakIterator;
import java.util.Locale;

public class BreakIteratorOracle {
    public static void main(String[] args) throws IOException {
        BufferedReader in = new BufferedReader(new InputStreamReader(System.in, "UTF-8"));
        for (String line; (line = in.readLine()) != null; ) {
            String[] parts = line.split("\t", -1);
            int[] points = parts[1].isEmpty() ? new int[0]
                : java.util.Arrays.stream(parts[1].split(",")).mapToInt(Integer::parseInt).toArray();
            String text = new String(points, 0, points.length);
            BreakIterator it = switch (parts[0]) {
                case "line" -> BreakIterator.getLineInstance(Locale.ROOT);
                case "word" -> BreakIterator.getWordInstance(Locale.ROOT);
                case "sentence" -> BreakIterator.getSentenceInstance(Locale.ROOT);
                default -> BreakIterator.getCharacterInstance(Locale.ROOT);
            };
            it.setText(text);
            StringBuilder out = new StringBuilder();
            for (int b = it.first(); b != BreakIterator.DONE; b = it.next()) {
                out.append(out.length() == 0 ? "" : ", ").append(b);
            }
            System.out.println(out);
        }
    }
}
"""


def unescape(text):
    """The test's Rust string literal (`\\u{..}`, `\\"`, `\\\\`) as code points."""
    out, i = [], 0
    while i < len(text):
        if text.startswith("\\u{", i):
            end = text.index("}", i)
            out.append(int(text[i + 3:end], 16))
            i = end + 1
        elif text[i] == "\\":
            out.append(ord({"n": "\n", "t": "\t", "r": "\r"}.get(text[i + 1], text[i + 1])))
            i += 2
        else:
            out.append(ord(text[i]))
            i += 1
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--java-home", help="the JDK to ask (default: javac and java on PATH)")
    parser.add_argument("--diff", action="store_true", help="print only the rows the test gets wrong")
    args = parser.parse_args()
    cases = [m.groups() for m in map(ROW.match, TEST.read_text(encoding="utf-8").splitlines()) if m]
    if not cases:
        sys.exit(f"no cases found in {TEST}")
    tool = lambda name: str(pathlib.Path(args.java_home, "bin", name)) if args.java_home else name
    with tempfile.TemporaryDirectory() as work:
        source = os.path.join(work, "BreakIteratorOracle.java")
        pathlib.Path(source).write_text(ORACLE, encoding="utf-8")
        subprocess.run([tool("javac"), "-nowarn", "-d", work, source], check=True)
        feed = "".join(f"{kind}\t{','.join(map(str, unescape(text)))}\n" for kind, text, _ in cases)
        answers = subprocess.run([tool("java"), "-cp", work, "BreakIteratorOracle"], input=feed, capture_output=True,
                                 text=True, encoding="utf-8", check=True).stdout.splitlines()
    differ = 0
    for (kind, text, expected), found in zip(cases, answers):
        if args.diff and found == expected.strip():
            continue
        differ += found != expected.strip()
        print(f'            ("{kind}", "{text}", &[{found}]),')
    if args.diff:
        print(f"{differ} of {len(cases)} rows differ from the JDK", file=sys.stderr)
        sys.exit(1 if differ else 0)


if __name__ == "__main__":
    main()
