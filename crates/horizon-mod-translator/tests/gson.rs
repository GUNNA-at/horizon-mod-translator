//! Gson as a mod uses it, against Gson itself: a fixture that writes and reads JSON through
//! `JsonWriter`, `JsonReader`, `Gson`, `JsonParser` and a `TypeAdapter` of its own runs on `java`
//! with the install's Gson, then is translated (Gson's streams taken from that jar as code, its
//! tree and `Gson` the runtime's own) and run again; every text must match.
//! Needs a JDK (`javac`, `java`, `jar`) and a NeoForge install; skipped without either.
use std::{fs, path::Path, process::Command};

/// (method, inputs): each is `static String name(int)`.
const CASES: &[(&str, &[&str])] = &[
    ("writer", &["0", "1", "2", "5", "7"]),
    ("writerErrors", &["0", "1", "2", "3", "4", "5"]),
    (
        "reader",
        &[
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15",
            "16", "17",
        ],
    ),
    ("typed", &["0", "1"]),
    ("gsonWriter", &["0", "1", "2", "4", "7"]),
    (
        "gsonReader",
        &["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"],
    ),
    ("adapter", &["0", "1"]),
    ("tree", &["0", "1", "2"]),
    ("reflective", &["0", "1"]),
    ("natural", &["0"]),
    ("tokens", &["0"]),
    ("adapters", &["0", "1", "2", "3", "4", "5"]),
    ("factories", &["0"]),
    ("policy", &["0", "1", "2", "3", "4", "5"]),
    ("renamedEnums", &["0"]),
];

const FIXTURE: &str = r##"
import com.google.gson.ExclusionStrategy;
import com.google.gson.FieldAttributes;
import com.google.gson.FieldNamingPolicy;
import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.InstanceCreator;
import com.google.gson.JsonDeserializationContext;
import com.google.gson.JsonDeserializer;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.JsonPrimitive;
import com.google.gson.JsonSerializationContext;
import com.google.gson.JsonSerializer;
import com.google.gson.TypeAdapter;
import com.google.gson.TypeAdapterFactory;
import com.google.gson.annotations.SerializedName;
import com.google.gson.reflect.TypeToken;
import com.google.gson.stream.JsonReader;
import com.google.gson.stream.JsonToken;
import com.google.gson.stream.JsonWriter;
import java.io.IOException;
import java.io.StringReader;
import java.io.StringWriter;
import java.lang.reflect.Modifier;
import java.lang.reflect.ParameterizedType;
import java.lang.reflect.Type;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public class GsonFixture {
    /// One line for a text of several.
    public static String esc(String s) {
        return s == null ? "null" : s.replace("\\", "\\\\").replace("\n", "\\n");
    }

    static String thrown(Exception e) {
        return e.getClass().getName() + ": " + e.getMessage();
    }

    public static String writer(int n) {
        StringWriter out = new StringWriter();
        try {
            JsonWriter w = new JsonWriter(out);
            if ((n & 1) != 0) w.setIndent("  ");
            if ((n & 2) != 0) w.setHtmlSafe(true);
            if ((n & 4) != 0) w.setSerializeNulls(false);
            w.beginObject();
            w.name("id").value("create:shaft");
            w.name("count").value(64);
            w.name("speed").value(2.5);
            w.name("ratio").value(0.1f);
            w.name("big").value(1234567890123L);
            w.name("on").value(true);
            w.name("boxed").value(Boolean.FALSE);
            w.name("none").nullValue();
            w.name("text").value((String) null);
            w.name("html").value("<a href='x'>&=</a>");
            w.name("escapes").value("tab\t quote\" back\\ nl\n \u0001 \u2028 \u00e9");
            w.name("number").value((Number) Integer.valueOf(7));
            w.name("whole").value((Number) Double.valueOf(3.0));
            w.name("list").beginArray().value(1).value("two").beginArray().endArray().beginObject().endObject().endArray();
            w.name("raw").jsonValue("{\"k\":1}");
            w.endObject();
            w.close();
        } catch (Exception e) {
            return thrown(e) + " | " + out.toString();
        }
        return out.toString();
    }

    public static String writerErrors(int n) {
        StringWriter out = new StringWriter();
        JsonWriter w = new JsonWriter(out);
        try {
            switch (n) {
                case 0: w.name("x"); break;
                case 1: w.beginObject(); w.value(1); break;
                case 2: w.beginArray(); w.value(Double.NaN); break;
                case 3: w.setLenient(true); w.beginArray(); w.value(Double.NaN); w.value(Float.NEGATIVE_INFINITY); w.endArray(); break;
                case 4: w.beginArray(); w.close(); break;
                default: w.value(1); w.value(2); break;
            }
        } catch (Exception e) {
            return thrown(e) + " | " + out.toString();
        }
        return out.toString();
    }

    static void walk(JsonReader r, StringBuilder sb) throws IOException {
        while (true) {
            JsonToken t = r.peek();
            sb.append(t).append('@').append(r.getPath());
            switch (t) {
                case BEGIN_ARRAY: r.beginArray(); break;
                case END_ARRAY: r.endArray(); break;
                case BEGIN_OBJECT: r.beginObject(); break;
                case END_OBJECT: r.endObject(); break;
                case NAME: sb.append('=').append(r.nextName()); break;
                case STRING: sb.append('=').append(r.nextString()); break;
                case NUMBER: sb.append('=').append(r.nextString()); break;
                case BOOLEAN: sb.append('=').append(r.nextBoolean()); break;
                case NULL: r.nextNull(); break;
                default: return;
            }
            sb.append(' ');
        }
    }

    static final String[] DOCS = {
        "{\"a\": 1, \"b\": [true, null, \"x\"], \"c\": {\"d\": -2.50e3}}",
        "[1, 2.0, \"3\", 1e2, -0, 12345678901234567890]",
        "// comment\n{name: 'single', other = unquoted; list: [1,,2]} # tail",
        "{\"a\": }",
        "[1, 2",
        "",
        "  \"just a string\"  ",
        "{\"a\": 1} trailing",
        "[01, +1, .5, NaN, Infinity]",
    };

    public static String reader(int n) {
        JsonReader r = new JsonReader(new StringReader(DOCS[n / 2]));
        r.setLenient((n & 1) != 0);
        StringBuilder sb = new StringBuilder();
        try {
            walk(r, sb);
        } catch (Exception e) {
            sb.append(" ! ").append(thrown(e));
        }
        return sb.toString();
    }

    public static String typed(int n) {
        JsonReader r = new JsonReader(new StringReader(
            "{\"i\": \"12\", \"d\": 3, \"l\": 9007199254740993, \"s\": 1.5, \"b\": true, \"skip\": {\"deep\": [1, 2, {\"x\": null}]},"
            + " \"z\": \"end\", \"f\": 1.0, \"big\": 1e400}"));
        r.setLenient(n == 1);
        StringBuilder sb = new StringBuilder();
        try {
            r.beginObject();
            while (r.hasNext()) {
                String name = r.nextName();
                switch (name) {
                    case "i": sb.append(r.nextInt()); break;
                    case "d": sb.append(r.nextDouble()); break;
                    case "l": sb.append(r.nextLong()); break;
                    case "s": sb.append(r.nextString()); break;
                    case "b": sb.append(r.nextBoolean()); break;
                    case "f": sb.append(r.nextInt()); break;
                    case "big": sb.append(r.nextDouble()); break;
                    case "skip": r.skipValue(); break;
                    default: sb.append(r.nextString()); break;
                }
                sb.append(',').append(r.getPath()).append(' ');
            }
            r.endObject();
            sb.append(r.peek());
        } catch (Exception e) {
            sb.append(" ! ").append(thrown(e));
        }
        return sb.toString();
    }

    public static String gsonWriter(int n) {
        JsonElement tree = JsonParser.parseString(
            "{\"name\": \"<gear>\", \"teeth\": 12, \"ratio\": 0.5, \"tags\": [\"a\", null, true], \"none\": null, \"nested\": {\"k\": []}}");
        GsonBuilder b = new GsonBuilder();
        if ((n & 1) != 0) b.setPrettyPrinting();
        if ((n & 2) != 0) b.disableHtmlEscaping();
        if ((n & 4) != 0) b.serializeNulls();
        Gson gson = b.create();
        StringWriter out = new StringWriter();
        try {
            JsonWriter w = new JsonWriter(out);
            w.setIndent(" ");
            gson.toJson(tree, w);
            w.flush();
            String first = out.toString();
            StringWriter typed = new StringWriter();
            gson.toJson(tree, JsonElement.class, new JsonWriter(typed));
            return first + " | " + w.isLenient() + w.isHtmlSafe() + w.getSerializeNulls() + " | " + typed.toString() + " | " + gson.toJson(tree);
        } catch (Exception e) {
            return thrown(e) + " | " + out.toString();
        }
    }

    public static String gsonReader(int n) {
        String[] docs = {"{\"a\": [1, 2.5, \"x\", null, {\"b\": false}]}", "", "[1, 2", "{a: 'lenient'}", "\"text\"", "{\"a\": 1} {\"b\": 2}"};
        String doc = docs[n % docs.length];
        try {
            JsonReader r = new JsonReader(new StringReader(doc));
            if (n < docs.length) {
                JsonElement e = new Gson().fromJson(r, JsonElement.class);
                return (e == null ? "null" : e.toString()) + " | " + r.isLenient();
            }
            JsonElement e = JsonParser.parseReader(r);
            return e.toString() + " | " + r.isLenient() + " | " + r.peek();
        } catch (Exception e) {
            return thrown(e);
        }
    }

    static class Point {
        int x;
        int y;
        String label;
        Point(int x, int y, String label) { this.x = x; this.y = y; this.label = label; }
    }

    static class PointAdapter extends TypeAdapter<Point> {
        @Override public void write(JsonWriter out, Point p) throws IOException {
            out.beginObject();
            out.name("at").value(p.x + "," + p.y);
            out.name("label").value(p.label);
            out.endObject();
        }

        @Override public Point read(JsonReader in) throws IOException {
            int x = 0;
            int y = 0;
            String label = null;
            in.beginObject();
            while (in.hasNext()) {
                switch (in.nextName()) {
                    case "at":
                        String[] parts = in.nextString().split(",");
                        x = Integer.parseInt(parts[0]);
                        y = Integer.parseInt(parts[1]);
                        break;
                    case "label":
                        if (in.peek() == JsonToken.NULL) {
                            in.nextNull();
                        } else {
                            label = in.nextString();
                        }
                        break;
                    default:
                        in.skipValue();
                }
            }
            in.endObject();
            return new Point(x, y, label);
        }
    }

    public static String adapter(int n) {
        try {
            PointAdapter a = new PointAdapter();
            Point p = new Point(3, -4, n == 0 ? "origin<ish>" : null);
            String text = a.toJson(p);
            JsonElement tree = a.toJsonTree(p);
            Point back = a.fromJson(text);
            Point fromTree = a.fromJsonTree(tree);
            TypeAdapter<Point> safe = a.nullSafe();
            return text + " | " + tree.toString() + " | " + back.x + "," + back.y + "," + back.label + " | " + fromTree.x + fromTree.label
                + " | " + safe.toJson(null) + " | " + safe.fromJson("null") + " | " + safe.toJsonTree(null).toString();
        } catch (Exception e) {
            return thrown(e);
        }
    }

    enum Kind { SPUR, BEVEL }

    enum Version {
        @SerializedName("1.12.0") V_1_12_0,
        @SerializedName(value = "1.14.0", alternate = {"1.14", "latest"}) V_1_14_0,
        PLAIN;
    }

    static class Model {
        Version format_version;
        Version[] seen;
    }

    public static String renamedEnums(int n) {
        try {
            Gson gson = new Gson();
            Model m = gson.fromJson("{\"format_version\": \"1.12.0\", \"seen\": [\"latest\", \"PLAIN\", \"V_1_12_0\", \"1.14\", \"nothing\"]}", Model.class);
            String seen = "";
            for (Version v : m.seen) seen += v + ",";
            return m.format_version + " " + seen + " " + gson.toJson(m) + " " + gson.toJson(Version.V_1_14_0) + " " + gson.fromJson("\"1.14.0\"", Version.class);
        } catch (Exception e) {
            return thrown(e);
        }
    }

    static class Gear {
        String name = "unnamed";
        int teeth;
        double ratio = 1.0;
        boolean on;
        List<String> tags = new ArrayList<>();
        Map<String, Integer> counts = new LinkedHashMap<>();
        Point at;
        Point[] path;
        transient int cache = 5;
        static int SHARED = 1;
        Kind kind;
        Gear() {}
    }

    static String show(Point p) {
        return p == null ? "null" : p.x + ":" + p.y + ":" + p.label;
    }

    static String show(Gear g) {
        return g.name + "/" + g.teeth + "/" + g.ratio + "/" + g.on + "/" + g.tags + "/" + g.counts + "/" + show(g.at) + "/"
            + (g.path == null ? "null" : g.path.length + show(g.path[0])) + "/" + g.cache + "/" + g.kind;
    }

    public static String reflective(int n) {
        try {
            Gson gson = n == 1 ? new GsonBuilder().serializeNulls().setPrettyPrinting().create() : new Gson();
            String text = "{\"name\": \"big\", \"teeth\": 12, \"ratio\": 0.25, \"on\": true, \"tags\": [\"a\", \"b\"], \"counts\": {\"x\": 1, \"y\": 20},"
                + " \"at\": {\"x\": 3, \"y\": 4, \"label\": \"c\"}, \"path\": [{\"x\": 1}, null], \"cache\": 9, \"kind\": \"BEVEL\", \"unknown\": [1]}";
            Gear g = gson.fromJson(text, Gear.class);
            Gear empty = gson.fromJson("{}", Gear.class);
            return show(g) + " | " + gson.toJson(g) + " | " + show(empty) + " | " + gson.toJson(empty) + " | "
                + gson.toJsonTree(g).getAsJsonObject().get("counts").toString();
        } catch (Exception e) {
            return thrown(e);
        }
    }

    public static String natural(int n) {
        try {
            Gson gson = new Gson();
            String text = "{\"a\": 1, \"b\": [1, 2.5, \"x\", true, null, {\"c\": 10000000000}], \"d\": {\"e\": -0.0}}";
            Object any = gson.fromJson(text, Object.class);
            Map<?, ?> map = gson.fromJson(text, Map.class);
            List<?> list = gson.fromJson("[1, [2], {\"k\": 3}]", List.class);
            Object first = ((Map<?, ?>) any).get("a");
            return any + " | " + map.get("b") + " | " + list + " | " + first.getClass().getSimpleName() + " | " + gson.toJson(any) + " | " + gson.toJson(list);
        } catch (Exception e) {
            return thrown(e);
        }
    }

    public static String tokens(int n) {
        try {
            Gson gson = new Gson();
            Type listOfPoints = new TypeToken<List<Point>>() {}.getType();
            Type mapType = new TypeToken<Map<String, List<Integer>>>() {}.getType();
            List<Point> points = gson.fromJson("[{\"x\": 1, \"y\": 2, \"label\": \"a\"}, {\"x\": 3}]", listOfPoints);
            Map<String, List<Integer>> m = gson.fromJson("{\"a\": [1, 2], \"b\": []}", mapType);
            int sum = 0;
            for (Point p : points) sum += p.x + p.y;
            for (List<Integer> l : m.values()) for (int i : l) sum += i;
            ParameterizedType pt = (ParameterizedType) mapType;
            TypeToken<?> made = TypeToken.getParameterized(Map.class, String.class, Point.class);
            return sum + " | " + points.get(0).label + points.size() + " | " + m + " | " + listOfPoints.getTypeName() + " | " + pt.getRawType().getTypeName()
                + " | " + pt.getActualTypeArguments()[1].getTypeName() + " | " + new TypeToken<List<Point>>() {}.getRawType().getName() + " | "
                + made.getType().getTypeName() + " | " + gson.toJson(points, listOfPoints) + " | " + (listOfPoints instanceof Class)
                + made.equals(TypeToken.getParameterized(Map.class, String.class, Point.class)) + (mapType instanceof ParameterizedType);
        } catch (Exception e) {
            return thrown(e);
        }
    }

    static class PointJson implements JsonSerializer<Point>, JsonDeserializer<Point> {
        @Override public JsonElement serialize(Point p, Type type, JsonSerializationContext context) {
            JsonObject o = new JsonObject();
            o.addProperty("p", p.x + ";" + p.y);
            o.add("label", context.serialize(p.label));
            o.addProperty("type", type.getTypeName());
            return o;
        }

        @Override public Point deserialize(JsonElement json, Type type, JsonDeserializationContext context) {
            JsonObject o = json.getAsJsonObject();
            String[] xy = o.get("p").getAsString().split(";");
            String label = context.deserialize(o.get("label"), String.class);
            return new Point(Integer.parseInt(xy[0]), Integer.parseInt(xy[1]), label);
        }
    }

    static class Holder {
        Point main;
        List<Point> all = new ArrayList<>();
        Object any;
        Kind kind = Kind.SPUR;
    }

    public static String adapters(int n) {
        try {
            GsonBuilder b = new GsonBuilder();
            String lone = "{\"x\": 1, \"label\": \"q\"}";
            switch (n) {
                case 0: b.registerTypeAdapter(Point.class, new PointJson()); lone = "{\"p\": \"1;2\", \"label\": \"q\"}"; break;
                case 1: b.registerTypeAdapter(Point.class, new PointAdapter()); lone = "{\"at\": \"1,2\", \"label\": \"q\"}"; break;
                case 2:
                    b.registerTypeAdapter(Point.class, new PointJson()).registerTypeAdapter(Point.class, new PointAdapter());
                    lone = "{\"at\": \"1,2\", \"label\": \"q\"}";
                    break;
                case 3: b.registerTypeHierarchyAdapter(Point.class, new PointJson()); lone = "{\"p\": \"1;2\", \"label\": \"q\"}"; break;
                case 4: b.registerTypeAdapter(Kind.class, (JsonSerializer<Kind>) (k, t, c) -> new JsonPrimitive(k.name().toLowerCase())); break;
                default: b.registerTypeAdapter(Point.class, (InstanceCreator<Point>) t -> new Point(7, 7, "made")); break;
            }
            Gson gson = b.create();
            Holder h = new Holder();
            h.main = new Point(1, 2, "m");
            h.all.add(new Point(3, 4, null));
            h.any = new Point(5, 6, "any");
            String text = gson.toJson(h);
            Holder back = n == 4 ? gson.fromJson("{\"kind\": \"BEVEL\"}", Holder.class) : gson.fromJson(text, Holder.class);
            return text + " | " + show(back.main) + " " + back.all.size() + (back.all.isEmpty() ? "" : show(back.all.get(0))) + " " + back.any + " " + back.kind
                + " | " + gson.toJson(new Point(8, 9, "d")) + " | " + show(gson.fromJson(lone, Point.class)) + " | " + gson.toJson(Kind.BEVEL);
        } catch (Exception e) {
            return thrown(e);
        }
    }

    static class ViaFactory implements TypeAdapterFactory {
        @Override public <T> TypeAdapter<T> create(Gson gson, TypeToken<T> type) {
            if (type.getRawType() != Point.class) {
                return null;
            }
            final TypeAdapter<T> delegate = gson.getDelegateAdapter(this, type);
            final TypeAdapter<JsonElement> elements = gson.getAdapter(JsonElement.class);
            return new TypeAdapter<T>() {
                @Override public void write(JsonWriter out, T value) throws IOException {
                    JsonElement tree = delegate.toJsonTree(value);
                    if (tree.isJsonObject()) {
                        tree.getAsJsonObject().addProperty("via", "factory");
                    }
                    elements.write(out, tree);
                }

                @Override public T read(JsonReader in) throws IOException {
                    JsonElement tree = elements.read(in);
                    if (tree.isJsonObject()) {
                        tree.getAsJsonObject().addProperty("label", "read");
                    }
                    return delegate.fromJsonTree(tree);
                }
            };
        }
    }

    public static String factories(int n) {
        try {
            Gson gson = new GsonBuilder().registerTypeAdapterFactory(new ViaFactory()).create();
            Holder h = new Holder();
            h.main = new Point(1, 2, "m");
            h.all.add(new Point(3, 4, "x"));
            String text = gson.toJson(h);
            Holder back = gson.fromJson(text, Holder.class);
            TypeAdapter<Point> a = gson.getAdapter(Point.class);
            TypeAdapter<Holder> plain = gson.getAdapter(Holder.class);
            return text + " | " + back.main.label + back.all.get(0).label + " | " + a.toJson(new Point(5, 6, null)) + " | " + plain.toJson(h) + " | "
                + plain.fromJson("{\"main\": {\"x\": 9}}").main.x + " | " + gson.getAdapter(new TypeToken<List<Point>>() {}).toJson(h.all);
        } catch (Exception e) {
            return thrown(e);
        }
    }

    static class Named {
        String displayName = "Gear Box";
        int toothCount = 12;
        transient int cached = 3;
        static int shared = 4;
        @SerializedName("explicit") String renamedField = "kept";
        Point origin = new Point(0, 0, "o");
        String secretKey = "hidden";
    }

    static class SkipSecrets implements ExclusionStrategy {
        @Override public boolean shouldSkipField(FieldAttributes f) {
            return f.getName().startsWith("secret") || f.hasModifier(Modifier.VOLATILE) || f.getAnnotation(Deprecated.class) != null;
        }

        @Override public boolean shouldSkipClass(Class<?> c) {
            return c == Point.class;
        }
    }

    public static String policy(int n) {
        try {
            GsonBuilder b = new GsonBuilder();
            switch (n) {
                case 0: b.setFieldNamingPolicy(FieldNamingPolicy.LOWER_CASE_WITH_UNDERSCORES); break;
                case 1: b.setFieldNamingPolicy(FieldNamingPolicy.UPPER_CAMEL_CASE_WITH_SPACES); break;
                case 2: b.excludeFieldsWithModifiers(Modifier.STATIC); break;
                case 3: b.setExclusionStrategies(new SkipSecrets()); break;
                case 4: b.addSerializationExclusionStrategy(new SkipSecrets()); break;
                default: b.setFieldNamingStrategy(f -> f.getName().toUpperCase() + "_" + f.getDeclaringClass().getSimpleName()); break;
            }
            Gson gson = b.create();
            String text = gson.toJson(new Named());
            Named back = gson.fromJson(
                "{\"display_name\": \"a\", \"Display Name\": \"b\", \"displayName\": \"c\", \"DISPLAYNAME_Named\": \"d\", \"explicit\": \"e\","
                + " \"cached\": 9, \"secretKey\": \"s\", \"origin\": {\"x\": 5}}", Named.class);
            return text + " | " + back.displayName + back.renamedField + back.cached + back.secretKey + show(back.origin);
        } catch (Exception e) {
            return thrown(e);
        }
    }

    public static String tree(int n) {
        String[] docs = {
            "{\"a\": 1.50, \"b\": 1e3, \"c\": -0, \"d\": 12345678901234567890, \"e\": 0.1, \"f\": 100, \"g\": 1E-2}",
            "[\"\\u00e9\\n\", \"<>&='\", \"\\u2028\"]",
            "{\"dup\": 1, \"dup\": 2, \"z\": {}, \"y\": []}",
        };
        try {
            JsonElement e = JsonParser.parseString(docs[n]);
            StringBuilder sb = new StringBuilder(e.toString());
            sb.append(" | ").append(new Gson().toJson(e)).append(" | ").append(new GsonBuilder().setPrettyPrinting().create().toJson(e));
            if (n == 0) {
                JsonObject o = e.getAsJsonObject();
                sb.append(" | ").append(o.get("a").getAsString()).append(' ').append(o.get("a").getAsInt()).append(' ').append(o.get("b").getAsInt())
                    .append(' ').append(o.get("b").getAsDouble()).append(' ').append(o.get("e").getAsFloat()).append(' ').append(o.get("f").getAsNumber())
                    .append(' ').append(o.get("g").getAsString()).append(' ').append(o.get("c").getAsString());
            }
            return sb.toString();
        } catch (Exception e) {
            return thrown(e);
        }
    }
}
"##;

fn has(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn translated_gson_matches_the_jvm() {
    if !has("javac") || !has("java") {
        eprintln!("no JDK: skipped");
        return;
    }
    let installed = horizon_mod_translator::find_game_jars().unwrap_or_default();
    let Some(gson) = installed.iter().find(|j| {
        j.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("gson-"))
    }) else {
        eprintln!("no NeoForge install with Gson: skipped");
        return;
    };
    let root = std::env::temp_dir().join(format!("mod-translator-gson-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let classes = root.join("classes");
    fs::create_dir_all(&classes).unwrap();
    fs::write(root.join("GsonFixture.java"), FIXTURE).unwrap();

    // The JVM's answers, with Gson's own classes.
    let mut runner = String::from("public class Runner { public static void main(String[] a) {\n");
    for (name, inputs) in CASES {
        for input in *inputs {
            runner.push_str(&format!(
                "System.out.println(\"{name}({input}) = \" + GsonFixture.esc(GsonFixture.{name}({input})));\n"
            ));
        }
    }
    runner.push_str("}}\n");
    fs::write(root.join("Runner.java"), runner).unwrap();
    let javac = Command::new("javac")
        .arg("-cp")
        .arg(gson)
        .args(["-d", classes.to_str().unwrap()])
        .arg(root.join("GsonFixture.java"))
        .arg(root.join("Runner.java"))
        .status()
        .unwrap();
    assert!(javac.success());
    let path = std::env::join_paths([classes.as_path(), gson.as_path()]).unwrap();
    let java = Command::new("java")
        .arg("-cp")
        .arg(&path)
        .arg("Runner")
        .output()
        .unwrap();
    assert!(
        java.status.success(),
        "{}",
        String::from_utf8_lossy(&java.stderr)
    );
    let expected = String::from_utf8(java.stdout).unwrap();

    // The mod's jar: the fixture alone. Gson is the install's, as a bundle takes it.
    let packed = root.join("packed");
    fs::create_dir_all(&packed).unwrap();
    for e in fs::read_dir(&classes).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with("GsonFixture") {
            fs::copy(e.path(), packed.join(e.file_name())).unwrap();
        }
    }
    let jar = root.join("fixture.jar");
    assert!(
        Command::new("jar")
            .arg("cf")
            .arg(&jar)
            .arg("-C")
            .arg(&packed)
            .arg(".")
            .status()
            .unwrap()
            .success()
    );
    let parsed = horizon_mod_translator::program_with(&[&jar], &[], &[gson.as_path()])
        .unwrap()
        .0;
    for class in [
        "com/google/gson/stream/JsonWriter",
        "com/google/gson/stream/JsonReader",
        "com/google/gson/TypeAdapter",
        "com/google/gson/internal/bind/JsonTreeWriter",
    ] {
        assert!(
            parsed.iter().any(|c| c.name == class),
            "{class} is part of the program"
        );
    }
    assert!(
        !parsed
            .iter()
            .any(|c| c.name == "com/google/gson/JsonObject"),
        "the tree stays the runtime's"
    );
    let mut program = horizon_mod_translator::codegen::Program::new(&parsed);
    let module = program.emit();

    // The generated crate.
    let krate = root.join("gen");
    fs::create_dir_all(krate.join("src")).unwrap();
    let compat = Path::new(env!("CARGO_MANIFEST_DIR")).join("../horizon-compat");
    fs::write(
        krate.join("Cargo.toml"),
        format!(
            "[package]\nname = \"gen\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\nhorizon-compat = {{ path = {:?} }}\n",
            compat.canonicalize().unwrap()
        ),
    )
    .unwrap();
    let mut main = String::from(
        "mod fixture;\nuse fixture::*;\nuse horizon_compat::{Rt, V};\nfn main() {\n    horizon_compat::install();\n    let mut rt = Rt::new();\n    init(&mut rt);\n    rt.reflect(\"GsonFixture\", \"<clinit>\", &[]);\n",
    );
    for (name, inputs) in CASES {
        let f =
            horizon_mod_translator::codegen::fn_name("GsonFixture", name, "(I)Ljava/lang/String;");
        for input in *inputs {
            main.push_str(&format!(
                "    let r = rt.entered(|rt| {f}(rt, &[V::I({input})]), |r| *r);\n    let text = rt.str_of(r).unwrap_or(\"null\").replace('\\\\', \"\\\\\\\\\").replace('\\n', \"\\\\n\");\n    println!(\"{name}({input}) = {{text}}\");\n"
            ));
            main.push_str(&format!("    if let Some(t) = rt.thrown.take() {{ eprintln!(\"{name}({input}) threw {{}}\", rt.class_of(t)); }}\n"));
        }
    }
    main.push_str("    let mut unmapped: Vec<_> = rt.unmapped.keys().collect();\n    unmapped.sort();\n    for k in unmapped { eprintln!(\"unmapped {k}\"); }\n");
    main.push_str("}\n");
    fs::write(krate.join("src/main.rs"), main).unwrap();
    fs::write(krate.join("src/fixture.rs"), &module).unwrap();
    let out = Command::new(env!("CARGO"))
        .args(["run", "-q", "--release", "--manifest-path"])
        .arg(krate.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "generated crate failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let actual = String::from_utf8(out.stdout).unwrap();
    let notes = String::from_utf8_lossy(&out.stderr);
    let mut wrong = Vec::new();
    for (e, a) in expected.lines().zip(actual.lines()) {
        if a != e {
            wrong.push(format!("{a}\n   (the JVM: {e})"));
        }
    }
    // `HZ_KEEP` leaves the generated crate where it is, for a look at it.
    if std::env::var_os("HZ_KEEP").is_some() {
        eprintln!("kept: {}", root.display());
    } else {
        fs::remove_dir_all(&root).unwrap();
    }
    assert!(wrong.is_empty(), "{}\n{notes}", wrong.join("\n"));
    assert_eq!(
        expected.lines().count(),
        CASES.iter().map(|c| c.1.len()).sum::<usize>(),
        "{expected}"
    );
    assert_eq!(expected.lines().count(), actual.lines().count(), "{notes}");
    assert!(!notes.contains("unmapped"), "{notes}");
    assert!(program.unsupported.is_empty(), "{:?}", program.unsupported);
}
