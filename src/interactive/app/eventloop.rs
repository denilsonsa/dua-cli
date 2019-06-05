use crate::interactive::{
    react::Terminal,
    sorted_entries,
    widgets::{ReactHelpPane, ReactMainWindow},
    ByteVisualization, DisplayOptions, EntryDataBundle, SortMode,
};
use dua::{
    path_of,
    traverse::{Traversal, TreeIndex},
    WalkOptions, WalkResult,
};
use failure::Error;
use itertools::Itertools;
use petgraph::Direction;
use std::{io, path::PathBuf};
use termion::input::{Keys, TermReadEventsAndRaw};
use tui::backend::Backend;

#[derive(Copy, Clone)]
pub enum FocussedPane {
    Main,
    Help,
}

impl Default for FocussedPane {
    fn default() -> Self {
        FocussedPane::Main
    }
}

#[derive(Default)]
pub struct AppState {
    pub root: TreeIndex,
    pub selected: Option<TreeIndex>,
    pub entries: Vec<EntryDataBundle>,
    pub sorting: SortMode,
    pub message: Option<String>,
    pub focussed: FocussedPane,
}

/// State and methods representing the interactive disk usage analyser for the terminal
pub struct TerminalApp {
    pub traversal: Traversal,
    pub display: DisplayOptions,
    pub state: AppState,
    pub window: ReactMainWindow,
}

enum CursorDirection {
    PageDown,
    Down,
    Up,
    PageUp,
}

impl TerminalApp {
    fn draw<B>(&mut self, terminal: &mut Terminal<B>) -> Result<(), Error>
    where
        B: Backend,
    {
        let mut window = self.window.clone(); // TODO: fix this - we shouldn't have to pass ourselves as props!
        terminal.render(&mut window, &*self, ())?;
        self.window = window;
        Ok(())
    }
    pub fn process_events<B, R>(
        &mut self,
        terminal: &mut Terminal<B>,
        keys: Keys<R>,
    ) -> Result<WalkResult, Error>
    where
        B: Backend,
        R: io::Read + TermReadEventsAndRaw,
    {
        use termion::event::Key::{Char, Ctrl};
        use FocussedPane::*;

        self.draw(terminal)?;
        for key in keys.filter_map(Result::ok) {
            self.update_message();
            match key {
                Char('?') => self.toggle_help_pane(),
                Char('\t') => {
                    self.cycle_focus();
                }
                Ctrl('c') => break,
                Char('q') => match self.state.focussed {
                    Main => break,
                    Help => {
                        self.state.focussed = Main;
                        self.window.help_pane = None
                    }
                },
                _ => {}
            }

            match self.state.focussed {
                FocussedPane::Help => match key {
                    Ctrl('u') => self.scroll_help(CursorDirection::PageUp),
                    Char('k') => self.scroll_help(CursorDirection::Up),
                    Char('j') => self.scroll_help(CursorDirection::Down),
                    Ctrl('d') => self.scroll_help(CursorDirection::PageDown),
                    _ => {}
                },
                FocussedPane::Main => match key {
                    Char('O') => self.open_that(),
                    Char('u') => self.exit_node(),
                    Char('o') => self.enter_node(),
                    Ctrl('u') => self.change_entry_selection(CursorDirection::PageUp),
                    Char('k') => self.change_entry_selection(CursorDirection::Up),
                    Char('j') => self.change_entry_selection(CursorDirection::Down),
                    Ctrl('d') => self.change_entry_selection(CursorDirection::PageDown),
                    Char('s') => self.state.sorting.toggle_size(),
                    Char('g') => self.display.byte_vis.cycle(),
                    _ => {}
                },
            };
            self.draw(terminal)?;
        }
        Ok(WalkResult {
            num_errors: self.traversal.io_errors,
        })
    }

    fn cycle_focus(&mut self) {
        use FocussedPane::*;
        self.state.focussed = match (self.state.focussed, &self.window.help_pane) {
            (Main, Some(_)) => Help,
            (Help, _) => Main,
            _ => Main,
        };
    }

    fn toggle_help_pane(&mut self) {
        use FocussedPane::*;
        self.state.focussed = match self.state.focussed {
            Main => {
                self.window.help_pane = Some(ReactHelpPane::default());
                Help
            }
            Help => {
                self.window.help_pane = None;
                Main
            }
        }
    }

    fn update_message(&mut self) {
        self.state.message = None;
    }

    fn open_that(&mut self) {
        match self.state.selected {
            Some(ref idx) => {
                open::that(path_of(&self.traversal.tree, *idx)).ok();
            }
            None => {}
        }
    }

    fn exit_node(&mut self) {
        match self
            .traversal
            .tree
            .neighbors_directed(self.state.root, Direction::Incoming)
            .next()
        {
            Some(parent_idx) => {
                self.state.root = parent_idx;
                self.state.entries =
                    sorted_entries(&self.traversal.tree, parent_idx, self.state.sorting);
                self.state.selected = self.state.entries.get(0).map(|b| b.index);
            }
            None => self.state.message = Some("Top level reached".into()),
        }
    }

    fn enter_node(&mut self) {
        if let Some(new_root) = self.state.selected {
            self.state.entries = sorted_entries(&self.traversal.tree, new_root, self.state.sorting);
            match self.state.entries.get(0) {
                Some(b) => {
                    self.state.root = new_root;
                    self.state.selected = Some(b.index);
                }
                None => self.state.message = Some("Entry is a file or an empty directory".into()),
            }
        }
    }

    fn scroll_help(&mut self, direction: CursorDirection) {
        use CursorDirection::*;
        if let Some(ref mut pane) = self.window.help_pane {
            pane.scroll = match direction {
                Down => pane.scroll.saturating_add(1),
                Up => pane.scroll.saturating_sub(1),
                PageDown => pane.scroll.saturating_add(10),
                PageUp => pane.scroll.saturating_sub(10),
            };
        }
    }

    fn change_entry_selection(&mut self, direction: CursorDirection) {
        let entries = sorted_entries(&self.traversal.tree, self.state.root, self.state.sorting);
        let next_selected_pos = match self.state.selected {
            Some(ref selected) => entries
                .iter()
                .find_position(|b| b.index == *selected)
                .map(|(idx, _)| match direction {
                    CursorDirection::PageDown => idx.saturating_add(10),
                    CursorDirection::Down => idx.saturating_add(1),
                    CursorDirection::Up => idx.saturating_sub(1),
                    CursorDirection::PageUp => idx.saturating_sub(10),
                })
                .unwrap_or(0),
            None => 0,
        };
        self.state.selected = entries
            .get(next_selected_pos)
            .or(entries.last())
            .map(|b| b.index)
            .or(self.state.selected)
    }

    pub fn initialize<B>(
        terminal: &mut Terminal<B>,
        options: WalkOptions,
        input: Vec<PathBuf>,
    ) -> Result<TerminalApp, Error>
    where
        B: Backend,
    {
        terminal.hide_cursor()?;
        let mut display_options: DisplayOptions = options.clone().into();
        display_options.byte_vis = ByteVisualization::Bar;
        let mut window = ReactMainWindow::default();

        let traversal = Traversal::from_walk(options, input, move |traversal| {
            let state = AppState {
                root: traversal.root_index,
                sorting: Default::default(),
                message: Some("-> scanning <-".into()),
                ..Default::default()
            };
            let app = TerminalApp {
                traversal: traversal.clone(), // TODO absolutely fix this! We should not rely on this anymore when done
                display: display_options,
                state,
                window: Default::default(),
            };
            terminal.render(&mut window, &app, ()).map_err(Into::into)
        })?;

        let sorting = Default::default();
        let root = traversal.root_index;
        let entries = sorted_entries(&traversal.tree, root, sorting);
        let selected = entries.get(0).map(|b| b.index);
        display_options.byte_vis = ByteVisualization::PercentageAndBar;
        Ok(TerminalApp {
            state: AppState {
                root,
                sorting,
                selected,
                entries,
                ..Default::default()
            },
            display: display_options,
            traversal,
            window: Default::default(),
        })
    }
}