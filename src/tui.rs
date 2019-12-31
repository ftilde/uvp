use crate::data::{
    add_to_active, add_to_available, iter_active, iter_available, remove_from_active,
    remove_from_available,
};
use rusqlite::Connection;
use signal_hook::iterator::Signals;
use unsegen::base::{Color, GraphemeCluster, StyleModifier, Window};
use unsegen::container::{Container, ContainerManager, ContainerProvider, HSplit, Leaf};
use unsegen::input::{Input, Key, NavigateBehavior};
use unsegen::widget::{
    builtin::{Column, LineLabel, Table, TableRow},
    ColDemand, Demand2D, RenderingHints, RowDemand, SeparatingStyle, Widget,
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

struct HighlightLabel {
    inner: LineLabel,
}

impl HighlightLabel {
    fn new(content: String) -> Self {
        HighlightLabel {
            inner: LineLabel::new(content),
        }
    }
}

impl Widget for HighlightLabel {
    fn space_demand(&self) -> Demand2D {
        self.inner.space_demand()
    }

    fn draw(&self, mut window: Window, hints: RenderingHints) {
        if hints.active {
            window.set_default_style(
                StyleModifier::new()
                    .invert(true)
                    .bold(true)
                    .apply_to_default(),
            );
        }
        self.inner.draw(window, hints)
    }
}

struct ActiveRow {
    title: HighlightLabel,
    time: HighlightLabel,
    padding: Padding,
    data: Active,
}

impl TableRow for ActiveRow {
    const COLUMNS: &'static [Column<ActiveRow>] = &[
        Column {
            access: |r| &r.title,
            access_mut: |r| &mut r.title,
            behavior: |_, i| Some(i),
        },
        Column {
            access: |r| &r.time,
            access_mut: |r| &mut r.time,
            behavior: |_, i| Some(i),
        },
        Column {
            access: |r| &r.padding,
            access_mut: |r| &mut r.padding,
            behavior: |_, i| Some(i),
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
            table: Table::new(
                SeparatingStyle::AlternatingStyle(
                    StyleModifier::new().bg_color(Color::ansi_rgb(0, 0, 0)),
                ),
                SeparatingStyle::Draw(GraphemeCluster::try_from('|').unwrap()),
                StyleModifier::new(),
            ),
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
                title: HighlightLabel::new(active.title.clone()),
                time: {
                    let label = if let Some(duration_secs) = active.duration_secs {
                        let progress_str = format_duration_secs(active.playbackpos);
                        let duration_str = format_duration_secs(duration_secs);
                        let percentage = (active.playbackpos / duration_secs * 100.0) as u32;
                        format!("{}/{} ({}%)", progress_str, duration_str, percentage)
                    } else {
                        format_duration_secs(active.playbackpos)
                    };

                    HighlightLabel::new(label)
                },
                padding: Padding,
                data: active,
            });
        }
    }
}

impl Widget for ActiveTable {
    fn space_demand(&self) -> Demand2D {
        self.table.space_demand()
    }

    fn draw(&self, window: Window, hints: RenderingHints) {
        self.table.draw(window, hints)
    }
}

impl Container<<Tui as ContainerProvider>::Parameters> for ActiveTable {
    fn input(
        &mut self,
        input: Input,
        sender: &mut <Tui as ContainerProvider>::Parameters,
    ) -> Option<Input> {
        input
            .chain((Key::Char('\n'), || {
                if let Some(row) = self.table.current_row_mut() {
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
            .finish()
    }
}

struct AvailableRow {
    title: HighlightLabel,
    duration: HighlightLabel,
    publication: HighlightLabel,
    data: Available,
}

impl TableRow for AvailableRow {
    const COLUMNS: &'static [Column<AvailableRow>] = &[
        Column {
            access: |r| &r.title,
            access_mut: |r| &mut r.title,
            behavior: |_, i| Some(i),
        },
        Column {
            access: |r| &r.duration,
            access_mut: |r| &mut r.duration,
            behavior: |_, i| Some(i),
        },
        Column {
            access: |r| &r.publication,
            access_mut: |r| &mut r.publication,
            behavior: |_, i| Some(i),
        },
    ];
}

struct AvailableTable {
    table: Table<AvailableRow>,
    deleted: Vec<Available>,
}

impl Container<<Tui as ContainerProvider>::Parameters> for AvailableTable {
    fn input(
        &mut self,
        input: Input,
        sender: &mut <Tui as ContainerProvider>::Parameters,
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
            .finish()
    }
}

impl Widget for AvailableTable {
    fn space_demand(&self) -> Demand2D {
        self.table.space_demand()
    }

    fn draw(&self, window: Window, hints: RenderingHints) {
        self.table.draw(window, hints)
    }
}

impl AvailableTable {
    fn with_available(available: impl Iterator<Item = Available>) -> Self {
        let mut tui = AvailableTable {
            table: Table::new(
                SeparatingStyle::AlternatingStyle(
                    StyleModifier::new().bg_color(Color::ansi_rgb(0, 0, 0)),
                ),
                SeparatingStyle::Draw(GraphemeCluster::try_from('|').unwrap()),
                StyleModifier::new(),
            ),
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
                title: HighlightLabel::new(available.title.clone()),
                duration: HighlightLabel::new(if let Some(t) = available.duration_secs {
                    format_duration_secs(t)
                } else {
                    "".to_owned()
                }),
                publication: HighlightLabel::new(available.publication.to_rfc3339()),
                data: available,
            });
        }
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
    type Parameters = std::sync::mpsc::SyncSender<TuiMsg>;
    type Index = TuiComponents;
    fn get<'a, 'b: 'a>(&'b self, index: &'a Self::Index) -> &'b dyn Container<Self::Parameters> {
        match index {
            &TuiComponents::Available => &self.available,
            &TuiComponents::Active => &self.active,
        }
    }
    fn get_mut<'a, 'b: 'a>(
        &'b mut self,
        index: &'a Self::Index,
    ) -> &'b mut dyn Container<Self::Parameters> {
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

pub fn run(conn: &Connection, mpv_binary: &str) -> Result<(), rusqlite::Error> {
    let stdout = std::io::stdout();
    let mut term = unsegen::base::Terminal::new(stdout.lock()).unwrap();
    let mut tui = Tui {
        active: ActiveTable::with_active(iter_active(&conn)?.into_iter()),
        available: AvailableTable::with_available(iter_available(&conn)?.into_iter()),
    };

    let layout = HSplit::new(vec![
        Box::new(Leaf::new(TuiComponents::Active)),
        Box::new(Leaf::new(TuiComponents::Available)),
    ]);
    let mut manager = ContainerManager::<Tui>::from_layout(Box::new(layout));

    let (signals_sender, tui_receiver) = std::sync::mpsc::sync_channel(0);

    let stdin_read_lock =
        std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let input_sender = signals_sender.clone();
    let stdin_read_lock_loop = stdin_read_lock.clone();
    let _input_handler = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let stdin = stdin.lock();
        for input in Input::read_all(stdin) {
            let input = input.unwrap();
            input_sender.send(Msg::Input(input)).unwrap();

            let &(ref lock, ref cvar) = &*stdin_read_lock_loop;
            let mut allowed_to_pass = lock.lock().unwrap();
            while !*allowed_to_pass {
                allowed_to_pass = cvar.wait(allowed_to_pass).unwrap();
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

        // Do not let input loop start another iteration just yet.
        // In case we want to play a video, mpv needs full access to the terminal, including stdin!
        {
            let mut pass = stdin_read_lock.0.lock().unwrap();
            *pass = false;
        }
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
                    tui.update(conn)?;
                }
                TuiMsg::Delete(url) => {
                    remove_from_active(conn, &url)?;
                    remove_from_available(conn, &url)?;
                    tui.update(conn)?;
                }
                TuiMsg::AddAvailable(a) => {
                    add_to_available(conn, None, &a)?;
                    tui.update(conn)?;
                }
                TuiMsg::AddActive(a) => {
                    add_to_active(conn, &a)?;
                    tui.update(conn)?;
                }
            }
        }
        {
            let mut pass = stdin_read_lock.0.lock().unwrap();
            *pass = true;
            stdin_read_lock.1.notify_one();
        }
    }
    Ok(())

    //TODO
    //fix tui layout
}
