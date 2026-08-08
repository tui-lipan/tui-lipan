//! Dense process monitor inspired by desktop system inspectors.

use tui_lipan::prelude::*;

const BG: Color = Color::Rgb(12, 15, 18);
const PANEL: Color = Color::Rgb(15, 19, 22);
const BORDER: Color = Color::Rgb(65, 76, 84);
const TEXT: Color = Color::Rgb(205, 214, 220);
const MUTED: Color = Color::Rgb(112, 128, 139);
const CYAN: Color = Color::Rgb(137, 211, 245);
const ORANGE: Color = Color::Rgb(239, 174, 67);
const GREEN: Color = Color::Rgb(116, 210, 137);
const SELECTED: Color = Color::Rgb(61, 56, 45);

struct ProcessMonitor;

struct State {
    selected: usize,
}

#[derive(Clone, Copy, Debug)]
enum Msg {
    Select(TableEvent),
    ScrollTo(usize),
}

impl Component for ProcessMonitor {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State { selected: 3 }
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        ctx.request_focus("process-table");
        None
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        VStack::new()
            .style(Style::new().bg(BG))
            .child(live_strip())
            .child(metrics(ctx.viewport().w))
            .child(processes(ctx))
            .child(graphs(ctx.viewport().h))
            .child(command_bar())
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Select(event) => ctx.state.selected = event.index,
            Msg::ScrollTo(index) => ctx.state.selected = index,
        }
        Update::full()
    }
}

fn live_strip() -> Element {
    Text::from_spans([
        Span::new(" LIVE ").fg(BG).bg(GREEN).bold(),
        Span::new("  DISPLAY PAUSED ").fg(BG).bg(ORANGE).bold(),
    ])
    .style(Style::new().bg(PANEL))
    .height(Length::Px(1))
    .width(Length::Flex(1))
    .into()
}

fn metrics(viewport_width: u16) -> Element {
    let cpu = if viewport_width < 110 {
        Text::from_spans([
            Span::new(" CPU Usage [").fg(MUTED),
            Span::new("████████").fg(CYAN),
            Span::new("] ").fg(MUTED),
            Span::new("6%\n").fg(TEXT).bold(),
            Span::new(" P-core ").fg(MUTED),
            Span::new("4166 MHz").fg(TEXT).bold(),
            Span::new("  E-core ").fg(MUTED),
            Span::new("1882 MHz\n").fg(TEXT).bold(),
            Span::new(" P/E  ─────▂▅▂──  ────").fg(TEXT),
        ])
    } else {
        Text::from_spans([
            Span::new("  CPU Usage [").fg(MUTED),
            Span::new("███████████").fg(CYAN),
            Span::new("                 ]  ").fg(MUTED),
            Span::new("6%\n").fg(TEXT).bold(),
            Span::new("  P-core ").fg(MUTED),
            Span::new("4166 MHz").fg(TEXT).bold(),
            Span::new("   E-core ").fg(MUTED),
            Span::new("1882 MHz\n").fg(TEXT).bold(),
            Span::new("  Per-core Usage (P/E)\n").fg(MUTED),
            Span::new("  P  ──────────▂▅▂────  E  ────").fg(TEXT),
        ])
    };

    HStack::new()
        .height(Length::Px(6))
        .child(
            monitor_frame("RAM/VRAM")
                .width(Length::Px(43))
                .child(Text::from_spans([
                    Span::new(" Physical Memory ").fg(MUTED),
                    Span::new("15,733 / 34,089 MB (46%)\n").fg(TEXT).bold(),
                    Span::new(" Committed       ").fg(MUTED),
                    Span::new("23,089 / 36,236 MB (64%)\n").fg(TEXT).bold(),
                    Span::new(" GPU Dedicated   ").fg(MUTED),
                    Span::new("1,840 / 8,406 MB (22%)\n").fg(CYAN).bold(),
                    Span::new(" GPU Shared      ").fg(MUTED),
                    Span::new("197 / 17,044 MB (1%)").fg(CYAN).bold(),
                ])),
        )
        .child(
            monitor_frame("NW/DISK")
                .width(Length::Px(19))
                .child(Text::from_spans([
                    Span::new(" Net Rx   ").fg(MUTED),
                    Span::new("0 Mbps\n").fg(TEXT).bold(),
                    Span::new(" Net Tx   ").fg(MUTED),
                    Span::new("0 Mbps\n").fg(TEXT).bold(),
                    Span::new(" Disk R   ").fg(MUTED),
                    Span::new("0 MB/s\n").fg(TEXT).bold(),
                    Span::new(" Disk W   ").fg(MUTED),
                    Span::new("0 MB/s").fg(TEXT).bold(),
                ])),
        )
        .child(monitor_frame("CPUS").child(cpu))
        .into()
}

fn processes(ctx: &Context<ProcessMonitor>) -> Element {
    let header = TableRow::new([
        "PID",
        "Process",
        "Private ↓",
        "GPU D",
        "WS Priv",
        "CPU%",
        "WS",
        "Thrd",
        "Hndl",
        "USER",
        "GDI",
        ".NET",
        "GPU S",
    ])
    .style(Style::new().fg(CYAN).bold().underline());

    let rows = [
        [
            "10800",
            "explorer.exe",
            "252.1 MB",
            "44.6 MB",
            "114.4 MB",
            "0.1%",
            "350.4 MB",
            "158",
            "6,197",
            "365",
            "200",
            "--",
            "2.2 MB",
        ],
        [
            "28416",
            "OneDrive.Sync.Serv",
            "243.2 MB",
            "--",
            "118.5 MB",
            "0.0%",
            "126.9 MB",
            "36",
            "751",
            "13",
            "10",
            "--",
            "--",
        ],
        [
            "1036", "dwm.exe", "233.4 MB", "231.0 MB", "59.0 MB", "0.2%", "111.2 MB", "73",
            "2,226", "1", "1", "--", "4.1 MB",
        ],
        [
            "9604",
            "memory-eater.exe",
            "221.9 MB",
            "--",
            "974.8 KB",
            "0.0%",
            "7.3 MB",
            "1",
            "63",
            "1",
            "0",
            "--",
            "--",
        ],
        [
            "18276",
            "Microsoft.CmdPal.L",
            "209.2 MB",
            "23.4 MB",
            "10.6 MB",
            "0.0%",
            "145.8 MB",
            "46",
            "1,500",
            "70",
            "117",
            "--",
            "622.6 KB",
        ],
        [
            "27672",
            "explorer.exe",
            "202.1 MB",
            "43.1 MB",
            "88.4 MB",
            "0.0%",
            "247.4 MB",
            "120",
            "3,942",
            "295",
            "223",
            "--",
            "3.8 MB",
        ],
        [
            "2712",
            "WindowsTerminal.ex",
            "199.8 MB",
            "55.4 MB",
            "115.3 MB",
            "0.1%",
            "205.2 MB",
            "55",
            "1,023",
            "52",
            "36",
            "--",
            "78.5 MB",
        ],
        [
            "8480",
            "chrome.exe",
            "197.1 MB",
            "--",
            "177.4 MB",
            "0.0%",
            "275.6 MB",
            "32",
            "486",
            "0",
            "0",
            "--",
            "--",
        ],
        [
            "2996",
            "chrome.exe",
            "190.8 MB",
            "--",
            "171.9 MB",
            "0.5%",
            "279.9 MB",
            "31",
            "490",
            "0",
            "0",
            "--",
            "--",
        ],
        [
            "18536",
            "chrome.exe",
            "189.4 MB",
            "--",
            "120.5 MB",
            "0.0%",
            "306.0 MB",
            "46",
            "2,367",
            "110",
            "169",
            "--",
            "--",
        ],
    ]
    .into_iter()
    .map(TableRow::new);

    monitor_frame_rich(
        RichText::new()
            .span(Span::new("PROCESSES").fg(TEXT).bold())
            .span(Span::new(" · 339 visible · all processes").fg(MUTED)),
    )
    .height(Length::Px(10))
    .child(
        Table::new()
            .header(header)
            .rows(rows)
            .widths(vec![
                ColumnWidth::Fill(11),
                ColumnWidth::Fill(18),
                ColumnWidth::Fill(10),
                ColumnWidth::Fill(9),
                ColumnWidth::Fill(10),
                ColumnWidth::Fill(7),
                ColumnWidth::Fill(10),
                ColumnWidth::Fill(7),
                ColumnWidth::Fill(7),
                ColumnWidth::Fill(7),
                ColumnWidth::Fill(7),
                ColumnWidth::Fill(6),
                ColumnWidth::Fill(8),
            ])
            .selection_symbol(Some(">> * "))
            .unselected_symbol(Some("     "))
            .selection_style(Style::new().bg(SELECTED).fg(TEXT).bold())
            .row_style_full_width(true)
            .style(Style::new().bg(PANEL).fg(TEXT))
            .selected(ctx.state.selected)
            .on_select(ctx.link().callback(Msg::Select))
            .on_scroll_to(ctx.link().callback(Msg::ScrollTo))
            .key("process-table"),
    )
    .into()
}

fn graphs(viewport_height: u16) -> Element {
    let memory = [
        48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0,
        48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 48.0, 61.0, 61.0, 85.0, 95.0,
        95.0, 95.0, 95.0, 95.0, 95.0, 95.0, 95.0, 104.0, 104.0, 125.0, 125.0, 143.0, 143.0, 143.0,
        143.0, 143.0, 143.0, 143.0, 143.0, 143.0, 143.0, 143.0, 143.0, 170.0, 180.0, 190.0, 201.0,
        212.0, 222.0, 222.0,
    ];

    let chart = Chart::new()
        .series([ChartSeries::new("Private", memory)
            .mode(ChartSeriesMode::Braille)
            .style(Style::new().fg(CYAN))])
        .x_axis(
            ChartAxis::new()
                .tick_labels(["22:40:00", "22:40:15", "22:40:30", "22:40:45", "22:40:59"])
                .style(Style::new().fg(MUTED)),
        )
        .y_axis(
            ChartAxis::new()
                .ticks(3)
                .range(0.0, 230.0)
                .style(Style::new().fg(MUTED)),
        )
        .show_grid(false)
        .show_legend(false)
        .style(Style::new().bg(PANEL).fg(TEXT))
        .axis_style(Style::new().fg(MUTED))
        .height(Length::Flex(1));

    let ledger_row = |time: &str, private: &str, delta: &str, color| {
        Span::new(format!(" {time:<8} {private:<11} {delta:>11}\n")).fg(color)
    };
    let mut ledger_spans = vec![ledger_row("Time", "Private", "Delta", MUTED)];
    if viewport_height >= 33 {
        ledger_spans.extend([
            ledger_row("22:40:43", "158,871,552", "+0", TEXT),
            ledger_row("22:40:44", "158,871,552", "+0", TEXT),
            ledger_row("22:40:45", "158,871,552", "+0", TEXT),
            ledger_row("22:40:46", "158,871,552", "+0", TEXT),
        ]);
    }
    ledger_spans.extend([
        ledger_row("22:40:47", "158,871,552", "+10,510,336", CYAN),
        ledger_row("22:40:48", "179,892,224", "+10,510,336", CYAN),
        ledger_row("22:40:49", "190,402,560", "+10,510,336", CYAN),
        ledger_row("22:40:50", "200,912,896", "+10,510,336", CYAN),
        ledger_row("22:40:51", "211,423,232", "+10,510,336", CYAN),
        ledger_row("22:40:52", "221,933,568", "+10,510,336", CYAN),
        Span::new(" Max: 221,933,568 @ 22:40:52\n").fg(MUTED),
        Span::new(" A: 95,809,536  B: 158,871,552\n").fg(ORANGE),
        Span::new(" B-A: +63,062,016 (+155%)").fg(TEXT).bold(),
    ]);

    let ledger = Text::from_spans(ledger_spans)
        .width(Length::Auto)
        .height(Length::Flex(1));

    monitor_frame_rich(
        RichText::new()
            .span(Span::new("GRAPHS").fg(TEXT).bold())
            .span(Span::new(" · Span 60s · Cursor 22:40:59  ").fg(MUTED))
            .span(Span::new("A 22:40:28").fg(ORANGE))
            .span(Span::new(" · ").fg(MUTED))
            .span(Span::new("B 22:40:43").fg(ORANGE)),
    )
    .child(HStack::new().child(chart).child(ledger))
    .into()
}

fn command_bar() -> Element {
    Text::from_spans([
        Span::new(" PROCESSES  ").fg(CYAN).bold(),
        Span::new("↑↓").fg(ORANGE).bold(),
        Span::new(" Select   ").fg(MUTED),
        Span::new("PgUp/PgDn").fg(ORANGE).bold(),
        Span::new(" Scroll   ").fg(MUTED),
        Span::new("Enter").fg(ORANGE).bold(),
        Span::new(" Inspect   ").fg(MUTED),
        Span::new("Space").fg(ORANGE).bold(),
        Span::new(" Track   ").fg(MUTED),
        Span::new("Ctrl+C").fg(ORANGE).bold(),
        Span::new(" Quit").fg(MUTED),
    ])
    .style(Style::new().bg(PANEL))
    .width(Length::Flex(1))
    .height(Length::Px(1))
    .into()
}

fn monitor_frame(title: &'static str) -> Frame {
    monitor_frame_rich(RichText::new().span(Span::new(title).fg(MUTED).bold()))
}

fn monitor_frame_rich(title: RichText) -> Frame {
    Frame::new()
        .header_left(title)
        .style(Style::new().bg(PANEL).fg(BORDER))
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Process Monitor")
        .screen_background(BG)
        .mount(ProcessMonitor)
        .run()
}
