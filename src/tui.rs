use crate::data::{
    add_to_active, add_to_available, iter_active, iter_available, remove_from_active,
    remove_from_available,
};
use crate::refresh;
use rusqlite::Connection;
use signal_hook::iterator::Signals;
use unsegen::base::{Color, GraphemeCluster, StyleModifier, Window};
use unsegen::container::{Container, ContainerManager, ContainerProvider, HSplit, Leaf};
use unsegen::input::ScrollBehavior;
use unsegen::input::{Input, Key, NavigateBehavior};
use unsegen::widget::{
    builtin::{Column, Table, TableRow},
    ColDemand, Demand2D, RenderingHints, RowDemand, SeparatingStyle, Widget, WidgetExt,
};

use chrono::Duration;

use crate::data::{Active, Available};

fn format_duration_secs(duration: f64) -> String {
    format_duration(Duration::milliseconds((duration * 1_000.0) as i64))
}
fn format_duration(mut duration: Duration) -> String {
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

struct Padding;
impl Widget for Padding {
    fn space_demand(&self) -> Demand2D {
        Demand2D {
            width: ColDemand::at_least(0),
            height: RowDemand::exact(0),
        }
    }

    fn draw(&self, _win: Window, _hints: RenderingHints) {}
}

fn highlight_active(mut window: Window, hints: RenderingHints) -> Window {
    if hints.active {
        window.set_default_style(
            StyleModifier::new()
                .invert(true)
                .bold(true)
                .apply_to_default(),
        );
    }
    window
}

struct ActiveRow {
    source: String,
    title: String,
    time: String,
    data: Active,
}

impl TableRow for ActiveRow {
    type BehaviorContext = ();
    const COLUMNS: &'static [Column<ActiveRow>] = &[
        Column {
            access: |r| Box::new(r.source.as_str().with_window(highlight_active)),
            behavior: |_, i, _| Some(i),
        },
        Column {
            access: |r| Box::new(r.title.as_str().with_window(highlight_active)),
            behavior: |_, i, _| Some(i),
        },
        Column {
            access: |r| {
                Box::new(r.time.as_str().with_window(highlight_active).with_demand(
                    |d: Demand2D| Demand2D {
                        width: ColDemand::at_least(d.width.min),
                        height: d.height,
                    },
                ))
            },
            behavior: |_, i, _| Some(i),
        },
    ];
}

struct ActiveTable {
    table: Table<ActiveRow>,
    deleted: Vec<Active>,
}

impl ActiveTable {
    fn with_active(active: impl Iterator<Item = Active>) -> Self {
        let mut tui = ActiveTable {
            table: Table::new(),
            deleted: Vec::new(),
        };
        tui.update(active);
        tui
    }

    fn update(&mut self, active: impl Iterator<Item = Active>) {
        let mut rows = self.table.rows_mut();
        rows.clear();
        for active in active {
            rows.push(ActiveRow {
                source: active
                    .feed_title
                    .as_deref()
                    .unwrap_or("External")
                    .to_owned(),
                title: active.title.as_deref().unwrap_or("Unknown").to_owned(),
                time: {
                    let label = if let Some(duration_secs) = active.duration_secs {
                        let progress_str = format_duration_secs(active.position_secs);
                        let duration_str = format_duration_secs(duration_secs);
                        let percentage = (active.position_secs / duration_secs * 100.0) as u32;
                        format!("{}/{} ({}%)", progress_str, duration_str, percentage)
                    } else {
                        format_duration_secs(active.position_secs)
                    };

                    label
                },
                data: active,
            });
        }
    }
}

impl Container<<Tui as ContainerProvider>::Context> for ActiveTable {
    fn input(
        &mut self,
        input: Input,
        sender: &mut <Tui as ContainerProvider>::Context,
    ) -> Option<Input> {
        input
            .chain((Key::Char('\n'), || {
                if let Some(row) = self.table.current_row() {
                    sender.send(TuiMsg::Play(row.data.url.clone())).unwrap();
                }
            }))
            .chain((Key::Char('d'), || {
                if let Some(row) = self.table.current_row() {
                    self.deleted.push(row.data.clone());
                    sender.send(TuiMsg::Delete(row.data.url.clone())).unwrap();
                }
            }))
            .chain((&[Key::Char('u'), Key::Delete][..], || {
                if let Some(a) = self.deleted.pop() {
                    sender.send(TuiMsg::AddActive(a)).unwrap();
                }
            }))
            .chain(
                NavigateBehavior::new(&mut self.table)
                    .up_on(Key::Char('k'))
                    .up_on(Key::Up)
                    .down_on(Key::Char('j'))
                    .down_on(Key::Down),
            )
            .chain(
                ScrollBehavior::new(&mut self.table)
                    .to_end_on(Key::Char('G'))
                    .to_beginning_on(Key::Char('g')),
            )
            .finish()
    }

    fn as_widget<'a>(&'a self) -> Box<dyn Widget + 'a> {
        Box::new(
            self.table
                .as_widget()
                .row_separation(SeparatingStyle::AlternatingStyle(
                    StyleModifier::new().bg_color(Color::ansi_rgb(0, 0, 0)),
                ))
                .col_separation(SeparatingStyle::Draw(
                    GraphemeCluster::try_from('|').unwrap(),
                )),
        )
    }
}

struct AvailableRow {
    source: String,
    title: String,
    duration: String,
    publication: String,
    data: Available,
}

impl TableRow for AvailableRow {
    type BehaviorContext = ();
    const COLUMNS: &'static [Column<AvailableRow>] = &[
        Column {
            access: |r| Box::new(r.source.as_str().with_window(highlight_active)),
            behavior: |_, i, _| Some(i),
        },
        Column {
            access: |r| Box::new(r.title.as_str().with_window(highlight_active)),
            behavior: |_, i, _| Some(i),
        },
        Column {
            access: |r| Box::new(r.duration.as_str().with_window(highlight_active)),
            behavior: |_, i, _| Some(i),
        },
        Column {
            access: |r| {
                Box::new(
                    r.publication
                        .as_str()
                        .with_window(highlight_active)
                        .with_demand(|d: Demand2D| Demand2D {
                            width: ColDemand::at_least(d.width.min),
                            height: d.height,
                        }),
                )
            },
            behavior: |_, i, _| Some(i),
        },
    ];
}

struct AvailableTable {
    table: Table<AvailableRow>,
    deleted: Vec<Available>,
}

impl AvailableTable {
    fn with_available(available: impl Iterator<Item = Available>) -> Self {
        let mut tui = AvailableTable {
            table: Table::new(),
            deleted: Vec::new(),
        };
        tui.update(available);
        tui
    }
    fn update(&mut self, available: impl Iterator<Item = Available>) {
        let mut rows = self.table.rows_mut();
        rows.clear();
        for available in available {
            rows.push(AvailableRow {
                source: available.feed.title.clone(),
                title: available.title.clone(),
                duration: if let Some(t) = available.duration_secs {
                    format_duration_secs(t)
                } else {
                    "".to_owned()
                },
                publication: available.publication.to_rfc3339(),
                data: available,
            });
        }
    }
}

impl Container<<Tui as ContainerProvider>::Context> for AvailableTable {
    fn input(
        &mut self,
        input: Input,
        sender: &mut <Tui as ContainerProvider>::Context,
    ) -> Option<Input> {
        input
            .chain((Key::Char('\n'), || {
                if let Some(row) = self.table.current_row() {
                    sender.send(TuiMsg::Play(row.data.url.clone())).unwrap();
                }
            }))
            .chain((Key::Char('d'), || {
                if let Some(row) = self.table.current_row() {
                    self.deleted.push(row.data.clone());
                    sender.send(TuiMsg::Delete(row.data.url.clone())).unwrap();
                }
            }))
            .chain((Key::Char('u'), || {
                if let Some(a) = self.deleted.pop() {
                    sender.send(TuiMsg::AddAvailable(a)).unwrap();
                }
            }))
            .chain(
                NavigateBehavior::new(&mut self.table)
                    .up_on(Key::Char('k'))
                    .up_on(Key::Up)
                    .down_on(Key::Char('j'))
                    .down_on(Key::Down),
            )
            .chain(
                ScrollBehavior::new(&mut self.table)
                    .to_end_on(Key::Char('G'))
                    .to_beginning_on(Key::Char('g')),
            )
            .finish()
    }

    fn as_widget<'a>(&'a self) -> Box<dyn Widget + 'a> {
        Box::new(
            self.table
                .as_widget()
                .row_separation(SeparatingStyle::AlternatingStyle(
                    StyleModifier::new().bg_color(Color::ansi_rgb(0, 0, 0)),
                ))
                .col_separation(SeparatingStyle::Draw(
                    GraphemeCluster::try_from('|').unwrap(),
                )),
        )
    }
}

enum Msg {
    Input(Input),
    Redraw,
}
enum TuiMsg {
    Play(String),
    Delete(String),
    AddActive(Active),
    AddAvailable(Available),
    Refresh,
}

struct Tui {
    active: ActiveTable,
    available: AvailableTable,
}
impl Tui {
    fn update(&mut self, conn: &Connection) -> Result<(), rusqlite::Error> {
        self.available.update(iter_available(conn)?.into_iter());
        self.active.update(iter_active(conn)?.into_iter());
        Ok(())
    }
}
impl ContainerProvider for Tui {
    type Context = std::sync::mpsc::SyncSender<TuiMsg>;
    type Index = TuiComponents;
    fn get<'a, 'b: 'a>(&'b self, index: &'a Self::Index) -> &'b dyn Container<Self::Context> {
        match index {
            &TuiComponents::Available => &self.available,
            &TuiComponents::Active => &self.active,
        }
    }
    fn get_mut<'a, 'b: 'a>(
        &'b mut self,
        index: &'a Self::Index,
    ) -> &'b mut dyn Container<Self::Context> {
        match index {
            &TuiComponents::Available => &mut self.available,
            &TuiComponents::Active => &mut self.active,
        }
    }
    const DEFAULT_CONTAINER: TuiComponents = TuiComponents::Active;
}
#[derive(Clone, PartialEq, Debug)]
enum TuiComponents {
    Available,
    Active,
}

enum InputLoopMsg {
    Continue,
}

pub fn run(conn: &Connection, mpv_binary: &str) -> Result<(), rusqlite::Error> {
    refresh(&conn)?;

    let stdout = std::io::stdout();
    let mut term = unsegen::base::Terminal::new(stdout.lock()).unwrap();
    let mut tui = Tui {
        active: ActiveTable::with_active(iter_active(&conn)?.into_iter()),
        available: AvailableTable::with_available(iter_available(&conn)?.into_iter()),
    };

    if tui.available.table.rows().is_empty() && tui.active.table.rows().is_empty() {
        eprintln!("Neither active nor available entries. Have you added any feeds, yet?");
        return Ok(());
    }

    let layout = HSplit::new(vec![
        (Box::new(Leaf::new(TuiComponents::Active)), 1.0),
        (Box::new(Leaf::new(TuiComponents::Available)), 1.0),
    ]);
    let mut manager = ContainerManager::<Tui>::from_layout(Box::new(layout));

    let (signals_sender, tui_receiver) = std::sync::mpsc::sync_channel(0);
    let (input_continue_sender, input_continue_receiver) = std::sync::mpsc::sync_channel(0);

    let input_sender = signals_sender.clone();
    let _input_handler = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let stdin = stdin.lock();
        for input in Input::read_all(stdin) {
            let input = input.unwrap();
            input_sender.send(Msg::Input(input)).unwrap();

            // We can only continue processing input once the tui main loop is done with the
            // current iteration in case mpv needs to take over the terminal.
            // For this reason we wait here for the continue message.
            if input_continue_receiver.recv().is_err() {
                break;
            }
        }
    });

    let signals = Signals::new(&[signal_hook::SIGWINCH]).unwrap();
    let _signal_handler = std::thread::spawn(move || {
        for signal in signals.forever() {
            match signal {
                signal_hook::SIGWINCH => {
                    if signals_sender.send(Msg::Redraw).is_err() {
                        break;
                    }
                }
                _ => unreachable!(),
            }
        }
    });
    let (mut work_sender, work_receiver) = std::sync::mpsc::sync_channel(1);

    let mut run = true;
    while run {
        {
            let win = term.create_root_window();
            manager.draw(
                win,
                &mut tui,
                StyleModifier::new().fg_color(Color::Yellow),
                RenderingHints::default(),
            );
        }
        term.present();

        let mut input_continue_msg = None;
        if let Ok(msg) = tui_receiver.recv() {
            match msg {
                Msg::Input(input) => {
                    input
                        .chain((Key::Char('q'), || run = false))
                        .chain((Key::Char('r'), || {
                            work_sender.send(TuiMsg::Refresh).unwrap()
                        }))
                        .chain(manager.active_container_behavior(&mut tui, &mut work_sender))
                        .chain(
                            NavigateBehavior::new(&mut manager.navigatable(&mut tui))
                                .left_on(Key::Char('h'))
                                .left_on(Key::Left)
                                .right_on(Key::Char('l'))
                                .right_on(Key::Right),
                        );
                    input_continue_msg = Some(InputLoopMsg::Continue);
                }
                Msg::Redraw => {}
            }
        }
        if let Ok(msg) = work_receiver.try_recv() {
            match msg {
                TuiMsg::Play(url) => {
                    term.on_main_screen(|| crate::mpv::play(conn, &url, mpv_binary))
                        .unwrap()?;
                    tui.update(conn)?;
                }
                TuiMsg::Refresh => {
                    refresh(conn)?;
                    tui.update(conn)?;
                }
                TuiMsg::Delete(url) => {
                    remove_from_active(conn, &url)?;
                    remove_from_available(conn, &url)?;
                    tui.update(conn)?;
                }
                TuiMsg::AddAvailable(a) => {
                    add_to_available(conn, &a)?;
                    tui.update(conn)?;
                }
                TuiMsg::AddActive(a) => {
                    add_to_active(conn, &a)?;
                    tui.update(conn)?;
                }
            }
        }
        if let Some(m) = input_continue_msg {
            input_continue_sender.send(m).unwrap();
        }

        // Avoid accidentally focusing empty table
        if tui.available.table.rows().is_empty() {
            manager.set_active(TuiComponents::Active);
        } else if tui.active.table.rows().is_empty() {
            manager.set_active(TuiComponents::Available);
        }
    }
    Ok(())
}
