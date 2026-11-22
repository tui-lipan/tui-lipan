import contextlib
import importlib.util
import io
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).resolve().parent / "check-export-parity.py"


def load_checker():
    spec = importlib.util.spec_from_file_location("check_export_parity", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_export_parity"] = module
    spec.loader.exec_module(module)
    return module


def run_checker(lib_rs: str, prelude_rs: str, widgets_rs: str) -> tuple[int, str]:
    checker = load_checker()
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        lib_path = root / "lib.rs"
        prelude_path = root / "prelude.rs"
        widgets_path = root / "widgets_mod.rs"
        lib_path.write_text(lib_rs, encoding="utf-8")
        prelude_path.write_text(prelude_rs, encoding="utf-8")
        widgets_path.write_text(widgets_rs, encoding="utf-8")

        checker.LIB_RS = lib_path
        checker.PRELUDE_RS = prelude_path
        checker.WIDGETS_MOD_RS = widgets_path

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            code = checker.main()
        return code, buf.getvalue()


class ExportParityTests(unittest.TestCase):
    def test_widgets_vs_root_same_name_different_type_fails(self):
        widgets_rs = """
            mod flowchart;
            pub use flowchart::Edge;
        """
        lib_rs = """
            mod style;
            mod widgets;
            pub use crate::style::Edge;
        """
        prelude_rs = """
            pub use crate::style::Edge;
        """
        code, output = run_checker(lib_rs, prelude_rs, widgets_rs)
        self.assertNotEqual(
            code, 0, f"expected mismatch failure for Edge, got exit 0:\n{output}"
        )
        self.assertIn("Edge", output)

    def test_prelude_vs_root_same_name_different_type_fails(self):
        widgets_rs = ""
        lib_rs = """
            mod core;
            mod other;
            mod widgets;
            pub use crate::core::NodeId;
        """
        prelude_rs = """
            pub use crate::other::NodeId;
        """
        code, output = run_checker(lib_rs, prelude_rs, widgets_rs)
        self.assertNotEqual(
            code, 0, f"expected mismatch failure for NodeId, got exit 0:\n{output}"
        )
        self.assertIn("NodeId", output)

    def test_unresolvable_reexport_path_fails(self):
        widgets_rs = ""
        lib_rs = """
            mod widgets;
            pub use crate::widgets::Ghost;
        """
        prelude_rs = """
            pub use crate::Ghost;
        """
        code, output = run_checker(lib_rs, prelude_rs, widgets_rs)
        self.assertNotEqual(
            code, 0, f"expected failure for unresolvable Ghost, got exit 0:\n{output}"
        )
        self.assertIn("Ghost", output)

    def test_valid_aliases_relative_widgets_and_reexport_chains_pass(self):
        widgets_rs = """
            mod button;
            mod gantt;
            pub use button::Button;
            pub use gantt::{GanttDate, GanttDate as DiagramGanttDate};
            pub use crate::core::terminal_metrics::TerminalGraphicsMetrics;
        """
        lib_rs = """
            mod automation;
            mod core;
            mod style;
            mod widgets;
            pub use widgets::Button;
            pub use crate::widgets::{DiagramGanttDate, GanttDate};
            pub use crate::core::terminal_metrics::TerminalGraphicsMetrics;
            pub use crate::automation::FocusDirection as AutomationFocusDirection;
            pub use crate::style::Edge;
            pub use tui_lipan_macro::ui;
            pub fn child() {}
            #[macro_export]
            macro_rules! mockup { () => {}; }
        """
        prelude_rs = """
            pub use crate::widgets::{Button, DiagramGanttDate};
            pub use crate::{
                GanttDate, TerminalGraphicsMetrics, child, mockup, ui,
            };
            pub use crate::automation::FocusDirection as AutomationFocusDirection;
            pub use crate::style::Edge;
        """
        code, output = run_checker(lib_rs, prelude_rs, widgets_rs)
        self.assertEqual(code, 0, f"expected pass for valid re-export chains:\n{output}")


if __name__ == "__main__":
    unittest.main()
