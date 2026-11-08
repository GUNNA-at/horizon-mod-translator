"""tools/translator_progress.py: the coverage reports it sums, the README blocks it rewrites, the names
it prints for missing methods."""

import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'translator_progress', Path(__file__).resolve().parent.parent / 'tools/translator_progress.py')
tp = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tp)

REPORT = """horizon-compat: saved data  not read (no world folder); it won't be saved over
  mapped: 90 of 100 API methods (90.0%), 950 of 1000 class uses (95.0%)
  missing    30 net/minecraft/data/recipes/ShapedRecipeBuilder.save.(Lnet/minecraft/data/recipes/RecipeOutput;)V
  missing    12 net/minecraft/client/Minecraft.getTextureManager.()Lnet/minecraft/client/renderer/texture/TextureManager;
  missing     8 net/neoforged/neoforge/client/model/generators/BlockModelBuilder.texture.(Ljava/lang/String;)Lnet/neoforged/neoforge/client/model/generators/ModelBuilder;
  libraries: mapped 40 of 50 library methods (80.0%), 400 of 410 class uses (97.6%)
  lib-missing     7 java/lang/reflect/Method.invoke.(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;
"""


class ReportTests(unittest.TestCase):
    def test_datagen_calls_leave_the_totals(self):
        rep = tp.parse_report(REPORT)
        # Two datagen-only methods (30 and 8 call sites) leave both totals; the game's one stays missing.
        self.assertEqual(rep['api'], (90, 98, 950, 962))
        self.assertEqual(rep['missing'], {
            'net/minecraft/client/Minecraft.getTextureManager.()Lnet/minecraft/client/renderer/texture/TextureManager;': 12})
        self.assertEqual(rep['libs'], (40, 50, 400, 410))
        self.assertEqual(list(rep['lib_missing'].values()), [7])

    def test_a_report_without_totals_is_a_stopped_jar(self):
        self.assertIsNone(tp.parse_report('error: no classes\n'))


class ReadmeTests(unittest.TestCase):
    def test_blocks_are_filled_in_place_however_often(self):
        text = ('intro\n<!-- progress:a -->\n<!-- /progress -->\n\nbetween\n\n'
                '<!-- progress:b -->\nold\n<!-- /progress -->\nend\n')
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / 'README.md'
            path.write_text(text)
            for _ in range(2):
                tp.fill_readme(path, {'a': 'A1\nA2', 'b': 'B'})
            self.assertEqual(path.read_text(), 'intro\n<!-- progress:a -->\nA1\nA2\n<!-- /progress -->\n\nbetween\n\n'
                                               '<!-- progress:b -->\nB\n<!-- /progress -->\nend\n')


class NameTests(unittest.TestCase):
    def test_methods_read_as_java_declares_them(self):
        self.assertEqual(
            tp.java_name('net/minecraft/world/entity/ai/goal/GoalSelector.addGoal.(ILnet/minecraft/world/entity/ai/goal/Goal;)V'),
            'GoalSelector.addGoal(int, Goal)')
        self.assertEqual(
            tp.java_name('net/neoforged/neoforge/client/gui/ConfigurationScreen.<init>.'
                         '(Lnet/neoforged/fml/ModContainer;Lnet/minecraft/client/gui/screens/Screen;)V'),
            'new ConfigurationScreen(ModContainer, Screen)')
        self.assertEqual(tp.java_name('java/lang/reflect/Method.invoke.(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;'),
                         'Method.invoke(Object, Object[])')
        self.assertEqual(tp.java_name('net/minecraft/world/level/Level$Inner.at.([[JD)V'), 'Level.Inner.at(long[][], double)')


if __name__ == '__main__':
    unittest.main()
