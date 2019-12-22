use unsegen::base::{Color, GraphemeCluster, StyleModifier, Window};
use unsegen::input::Input;
use unsegen::widget::builtin::{Column, LineLabel, Table, TableRow};
use unsegen::widget::{Demand2D, RenderingHints, SeparatingStyle, Widget};

use chrono::Duration;

use crate::data::Active;

pub struct Row {
    title: LineLabel,
    url: LineLabel,
    time: LineLabel,
}

impl TableRow for Row {
    const COLUMNS: &'static [Column<Row>] = &[
        Column {
            access: |r| &r.title,
            access_mut: |r| &mut r.title,
            behavior: |_, i| Some(i),
        },
        Column {
            access: |r| &r.url,
            access_mut: |r| &mut r.url,
            behavior: |_, i| Some(i),
        },
        Column {
            access: |r| &r.time,
            access_mut: |r| &mut r.time,
            behavior: |_, i| Some(i),
        },
    ];
}

pub struct Tui {
    pub table: Table<Row>,
}

fn format_time(mut duration: Duration) -> String {
    let prefix = if duration < Duration::zero() {
        duration = -duration;
        "-"
    } else {
        " "
    };
    let minutes = duration.num_minutes();
    duration = duration - Duration::minutes(minutes);
    let seconds = duration.num_seconds();
    duration = duration - Duration::seconds(seconds);
    let millis = duration.num_milliseconds();
    format!("{}{:>2}:{:02}.{:03}", prefix, minutes, seconds, millis)
}

impl Tui {
    pub fn with_active(active: impl Iterator<Item = Active>) -> Self {
        let mut tui = Tui {
            table: Table::new(
                SeparatingStyle::AlternatingStyle(
                    StyleModifier::new().bg_color(Color::ansi_rgb(0, 0, 0)),
                ),
                SeparatingStyle::Draw(GraphemeCluster::try_from('|').unwrap()),
                StyleModifier::new(),
            ),
        };
        {
            let mut rows = tui.table.rows_mut();
            for active in active {
                rows.push(Row {
                    title: LineLabel::new(active.title),
                    url: LineLabel::new(active.link),
                    time: LineLabel::new(format_time(Duration::milliseconds(
                        (active.playbackpos * 1_000.0) as i64,
                    ))),
                });
            }
        }
        tui
    }
}

impl Widget for Tui {
    fn space_demand(&self) -> Demand2D {
        self.table.space_demand()
    }

    fn draw(&self, window: Window, hints: RenderingHints) {
        self.table.draw(window, hints)
    }
}

enum Msg {
    Input(Input),
}

pub fn run(active: impl Iterator<Item = Active>) {
    let stdout = std::io::stdout();
    let mut term = unsegen::base::Terminal::new(stdout.lock()).unwrap();
    let tui = Tui::with_active(active);

    let (tui_sender, tui_receiver) = std::sync::mpsc::sync_channel(1);

    let _input_handler = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let stdin = stdin.lock();
        for input in Input::read_all(stdin) {
            let input = input.unwrap();
            tui_sender.send(Msg::Input(input)).unwrap();
        }
    });

    loop {
        {
            let win = term.create_root_window();
            tui.draw(
                win,
                unsegen::widget::widget::RenderingHints::new().active(true),
            );
        }
        term.present();
        if let Ok(msg) = tui_receiver.recv() {
            match msg {
                Msg::Input(_input) => {
                    break;
                }
            }
        }
    }
}
