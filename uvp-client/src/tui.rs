use std::collections::HashMap;

use crate::Theme;
use signal_hook::iterator::Signals;
use unsegen::base::{Color, GraphemeCluster, RowIndex, StyleModifier, Window};
use unsegen::container::{Container, ContainerManager, ContainerProvider, HSplit, Leaf};
use unsegen::input::{Behavior, EditBehavior, Input, Key, NavigateBehavior};
use unsegen::input::{ScrollBehavior, WriteBehavior};
use unsegen::widget::{
    builtin::{Column, PromptLine, Table, TableRow},
    ColDemand, Demand2D, RenderingHints, SeparatingStyle, Widget, WidgetExt,
};

use chrono::Duration;
use uvp_state::data::{Active, Available, Store};

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

struct ActiveTable<'t> {
    table: Table<ActiveRow>,
    deleted: Vec<Active>,
    theme: &'t Theme,
}

impl<'t> ActiveTable<'t> {
    fn with_active(active: impl Iterator<Item = Active>, theme: &'t Theme) -> Self {
        let mut tui = ActiveTable {
            table: Table::new(),
            deleted: Vec::new(),
            theme,
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

impl Container<<Tui<'_> as ContainerProvider>::Context> for ActiveTable<'_> {
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
        if self.table.current_row().is_none() {
            Box::new("No active entries")
        } else {
            Box::new(
                self.table
                    .as_widget()
                    .row_separation(SeparatingStyle::AlternatingStyle(
                        StyleModifier::new()
                            .bg_color(self.theme.alt_bg)
                            .fg_color(self.theme.alt_fg),
                    ))
                    .col_separation(SeparatingStyle::Draw(
                        GraphemeCluster::try_from('|').unwrap(),
                    ))
                    .with_window(move |mut w, _| {
                        w.set_default_style(
                            StyleModifier::new()
                                .fg_color(self.theme.primary_fg)
                                .bg_color(self.theme.primary_bg)
                                .apply_to_default(),
                        );
                        w
                    }),
            )
        }
    }
}

struct AvailableRow {
    source: String,
    title: String,
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

struct AvailableTable<'t> {
    table: Table<AvailableRow>,
    deleted: Vec<Available>,
    theme: &'t Theme,
}

impl<'t> AvailableTable<'t> {
    fn with_available(available: impl Iterator<Item = Available>, theme: &'t Theme) -> Self {
        let mut tui = AvailableTable {
            table: Table::new(),
            deleted: Vec::new(),
            theme,
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
                publication: available.publication.to_rfc3339(),
                data: available,
            });
        }
    }
}

impl Container<<Tui<'_> as ContainerProvider>::Context> for AvailableTable<'_> {
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
        if self.table.current_row().is_none() {
            Box::new("No available entries")
        } else {
            Box::new(
                self.table
                    .as_widget()
                    .row_separation(SeparatingStyle::AlternatingStyle(
                        StyleModifier::new()
                            .bg_color(self.theme.alt_bg)
                            .fg_color(self.theme.alt_fg),
                    ))
                    .col_separation(SeparatingStyle::Draw(
                        GraphemeCluster::try_from('|').unwrap(),
                    ))
                    .with_window(move |mut w, _| {
                        w.set_default_style(
                            StyleModifier::new()
                                .fg_color(self.theme.primary_fg)
                                .bg_color(self.theme.primary_bg)
                                .apply_to_default(),
                        );
                        w
                    }),
            )
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
    Redraw,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Mode {
    Normal,
    Filter,
}

struct FilterPromptBehavior<'b, 't> {
    tui: &'b mut Tui<'t>,
    work_sender: &'b mut std::sync::mpsc::SyncSender<TuiMsg>,
}

impl<'b, 't> FilterPromptBehavior<'b, 't> {
    fn new(tui: &'b mut Tui<'t>, work_sender: &'b mut std::sync::mpsc::SyncSender<TuiMsg>) -> Self {
        FilterPromptBehavior { tui, work_sender }
    }
}

impl Behavior for FilterPromptBehavior<'_, '_> {
    fn input(self, input: Input) -> Option<Input> {
        let prompt = self.tui.active_prompt.as_mut().unwrap();

        input
            .chain(
                EditBehavior::new(prompt)
                    .delete_backwards_on(Key::Backspace)
                    .delete_forwards_on(Key::Delete)
                    .go_to_beginning_of_line_on(Key::Home)
                    .go_to_end_of_line_on(Key::End)
                    .right_on(Key::Right)
                    .left_on(Key::Left)
                    .up_on(Key::Up)
                    .down_on(Key::Down),
            )
            .chain(WriteBehavior::new(prompt))
            .chain((Key::Esc, || self.tui.switch_mode(Mode::Normal)))
            .chain((Key::Char('\n'), || {
                self.tui.filter = Some(
                    self.tui
                        .active_prompt
                        .as_mut()
                        .unwrap()
                        .finish_line()
                        .to_owned(),
                );
                self.tui.switch_mode(Mode::Normal);
                self.work_sender.send(TuiMsg::Redraw).unwrap();
            }))
            .finish()
    }
}

struct Tui<'t> {
    mode: Mode,
    filter: Option<String>,
    active_prompt: Option<PromptLine>,
    mode_prompts: HashMap<Mode, PromptLine>,
    active: ActiveTable<'t>,
    available: AvailableTable<'t>,
}
impl Tui<'_> {
    fn update(&mut self, db: &dyn Store) -> Result<(), uvp_state::Error> {
        let filter = self.filter.as_ref();

        self.available.update(
            db.all_available()?.into_iter().filter(|r| {
                filter.map_or(true, |f| r.feed.title.contains(f) || r.title.contains(f))
            }),
        );
        self.active.update(db.all_active()?.into_iter().filter(|r| {
            filter.map_or(true, |f| {
                r.title.as_ref().map(|t| t.contains(f)).unwrap_or(false)
                    || r.feed_title
                        .as_ref()
                        .map(|ft| ft.contains(f))
                        .unwrap_or(false)
            })
        }));
        Ok(())
    }

    fn switch_mode(&mut self, mode: Mode) {
        match &self.mode {
            Mode::Normal => {}
            Mode::Filter => {
                self.mode_prompts
                    .insert(Mode::Filter, self.active_prompt.take().unwrap());
            }
        }

        self.mode = mode;

        match &self.mode {
            Mode::Normal => {}
            Mode::Filter => {
                self.active_prompt = self.mode_prompts.remove(&Mode::Filter);
            }
        }
    }
}

impl ContainerProvider for Tui<'_> {
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

pub fn run(store: &dyn Store, mpv_binary: &str, theme: &Theme) -> Result<(), crate::Error> {
    store.refresh()?;

    let mut tui = Tui {
        mode: Mode::Normal,
        filter: None,
        active_prompt: None,
        mode_prompts: [(
            Mode::Filter,
            PromptLine::with_prompt("filter > ".to_string()),
        )]
        .into(),
        active: ActiveTable::with_active(store.all_active()?.into_iter(), theme),
        available: AvailableTable::with_available(store.all_available()?.into_iter(), theme),
    };

    if tui.available.table.rows().is_empty() && tui.active.table.rows().is_empty() {
        eprintln!("Neither active nor available entries. Have you added any feeds, yet?");
        return Ok(());
    }

    let stdout = std::io::stdout();
    let mut term = unsegen::base::Terminal::new(stdout.lock()).unwrap();

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
            let mut win = term.create_root_window();

            if let Some(prompt_line) = &tui.active_prompt {
                let main_height = win.get_height() - 1;
                let (main, prompt) = win.split(RowIndex::new(main_height.into())).unwrap();
                prompt_line
                    .as_widget()
                    .draw(prompt, RenderingHints::default());
                win = main
            } else if let Some(filter) = &tui.filter {
                let main_height = win.get_height() - 1;
                let (main, prompt) = win.split(RowIndex::new(main_height.into())).unwrap();
                format!("Filter: {}", filter).draw(prompt, RenderingHints::default());
                win = main
            }

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
                    match tui.mode {
                        Mode::Normal => {
                            input
                                .chain((Key::Char('q'), || run = false))
                                .chain((Key::Char('r'), || {
                                    work_sender.send(TuiMsg::Redraw).unwrap()
                                }))
                                .chain((Key::Char('R'), || {
                                    work_sender.send(TuiMsg::Refresh).unwrap()
                                }))
                                .chain((Key::Char('f'), || tui.switch_mode(Mode::Filter)))
                                .chain((Key::Esc, || {
                                    tui.filter = None;
                                    work_sender.send(TuiMsg::Redraw).unwrap();
                                }))
                                .chain(
                                    manager.active_container_behavior(&mut tui, &mut work_sender),
                                )
                                .chain(
                                    NavigateBehavior::new(&mut manager.navigatable(&mut tui))
                                        .left_on(Key::Char('h'))
                                        .left_on(Key::Left)
                                        .right_on(Key::Char('l'))
                                        .right_on(Key::Right),
                                );
                        }
                        Mode::Filter => {
                            input.chain(FilterPromptBehavior::new(&mut tui, &mut work_sender));
                        }
                    }
                    input_continue_msg = Some(InputLoopMsg::Continue);
                }
                Msg::Redraw => {}
            }
        }
        if let Ok(msg) = work_receiver.try_recv() {
            match msg {
                TuiMsg::Play(url) => {
                    term.on_main_screen(|| crate::mpv::play(store, &url, mpv_binary))
                        .unwrap()?;
                    tui.update(store)?;
                }
                TuiMsg::Refresh => {
                    store.refresh()?;
                    tui.update(store)?;
                }
                TuiMsg::Redraw => {
                    tui.update(store)?;
                }
                TuiMsg::Delete(url) => {
                    store.remove_from_active(&url)?;
                    store.remove_from_available(&url)?;
                    tui.update(store)?;
                }
                TuiMsg::AddAvailable(a) => {
                    store.add_to_available(&a)?;
                    tui.update(store)?;
                }
                TuiMsg::AddActive(a) => {
                    store.add_to_active(&a)?;
                    tui.update(store)?;
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
