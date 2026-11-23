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


def run_checker(
    lib_rs: str,
    prelude_rs: str,
    widgets_rs: str,
    prelude_budget: int | None = None,
    widgets_index_md: str = "",
) -> tuple[int, str]:
    checker = load_checker()
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        lib_path = root / "lib.rs"
        prelude_path = root / "prelude.rs"
        widgets_path = root / "widgets_mod.rs"
        index_path = root / "widgets_index.md"
        lib_path.write_text(lib_rs, encoding="utf-8")
        prelude_path.write_text(prelude_rs, encoding="utf-8")
        widgets_path.write_text(widgets_rs, encoding="utf-8")
        index_path.write_text(widgets_index_md, encoding="utf-8")

        checker.LIB_RS = lib_path
        checker.PRELUDE_RS = prelude_path
        checker.WIDGETS_MOD_RS = widgets_path
        checker.WIDGETS_INDEX_MD = index_path
        if prelude_budget is not None:
            checker.PRELUDE_EXPORT_BUDGET = prelude_budget

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            code = checker.main()
        return code, buf.getvalue()


class ExportParityTests(unittest.TestCase):
    def test_same_name_exported_twice_fails(self):
        widgets_rs = "mod button;\npub use button::Button;\n"
        lib_rs = """
            #[cfg(feature = "a")]
            pub use crate::style::Button;
            #[cfg(not(feature = "a"))]
            pub use crate::widgets::Button;
        """
        code, output = run_checker(lib_rs, "", widgets_rs)
        self.assertEqual(code, 1)
        self.assertIn("exports `Button` more than once", output)

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

    def test_prelude_at_budget_passes(self):
        lib_rs = """
            mod widgets;
            pub use crate::widgets::{Alpha, Beta};
        """
        widgets_rs = """
            mod a;
            pub use a::{Alpha, Beta};
        """
        prelude_rs = """
            pub use crate::widgets::{Alpha, Beta};
        """
        code, output = run_checker(lib_rs, prelude_rs, widgets_rs, prelude_budget=2)
        self.assertEqual(code, 0, f"expected pass at exact prelude budget:\n{output}")
        self.assertIn("2/2 prelude exports", output)

    def test_prelude_over_budget_fails_with_guidance(self):
        lib_rs = """
            mod widgets;
            pub use crate::widgets::{Alpha, Beta, Gamma};
        """
        widgets_rs = """
            mod a;
            pub use a::{Alpha, Beta, Gamma};
        """
        prelude_rs = """
            pub use crate::widgets::{Alpha, Beta, Gamma};
        """
        code, output = run_checker(lib_rs, prelude_rs, widgets_rs, prelude_budget=2)
        self.assertEqual(code, 1, f"expected failure over prelude budget:\n{output}")
        self.assertIn("exports 3 items, exceeding the budget of 2", output)
        self.assertIn("crate root (src/lib.rs)", output)
        self.assertIn("PRELUDE_EXPORT_BUDGET", output)

    def test_indexed_widget_missing_from_prelude_fails(self):
        widgets_rs = """
            mod button;
            mod heatmap;
            pub use button::Button;
            pub use heatmap::Heatmap;
        """
        lib_rs = """
            mod widgets;
            pub use crate::widgets::{Button, Heatmap};
        """
        prelude_rs = """
            pub use crate::widgets::Button;
        """
        index_md = """
            | Widget | Description |
            |--------|-------------|
            | `Button` | Interactive button |
            | `Heatmap` | 2D matrix |
        """
        code, output = run_checker(
            lib_rs, prelude_rs, widgets_rs, widgets_index_md=index_md
        )
        self.assertEqual(code, 1, f"expected failure for missing Heatmap:\n{output}")
        self.assertIn("docs/widgets/index.md", output)
        self.assertIn("Heatmap", output)

    def test_indexed_feature_gated_widget_in_prelude_passes(self):
        widgets_rs = """
            mod button;
            #[cfg(feature = "qr-code")]
            mod qr_code;
            pub use button::Button;
            #[cfg(feature = "qr-code")]
            pub use qr_code::QrCode;
        """
        lib_rs = """
            mod widgets;
            pub use crate::widgets::Button;
            #[cfg(feature = "qr-code")]
            pub use crate::widgets::QrCode;
        """
        prelude_rs = """
            pub use crate::widgets::Button;
            #[cfg(feature = "qr-code")]
            pub use crate::widgets::QrCode;
        """
        index_md = """
            | Widget | Feature | Description |
            |--------|---------|-------------|
            | `Button` | - | Interactive button |
            | `QrCode` | `qr-code` | Scannable QR symbol |
        """
        code, output = run_checker(
            lib_rs, prelude_rs, widgets_rs, widgets_index_md=index_md
        )
        self.assertEqual(code, 0, f"expected pass for feature-gated widget:\n{output}")

    def test_indexed_feature_gated_widget_missing_from_prelude_fails(self):
        widgets_rs = """
            #[cfg(feature = "qr-code")]
            mod qr_code;
            #[cfg(feature = "qr-code")]
            pub use qr_code::QrCode;
        """
        lib_rs = """
            mod widgets;
            #[cfg(feature = "qr-code")]
            pub use crate::widgets::QrCode;
        """
        prelude_rs = ""
        index_md = """
            | Widget | Feature | Description |
            |--------|---------|-------------|
            | `QrCode` | `qr-code` | Scannable QR symbol |
        """
        code, output = run_checker(
            lib_rs, prelude_rs, widgets_rs, widgets_index_md=index_md
        )
        self.assertEqual(
            code, 1, f"expected failure for missing feature-gated QrCode:\n{output}"
        )
        self.assertIn("QrCode", output)


if __name__ == "__main__":
    unittest.main()
