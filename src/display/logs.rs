// Copyright (C) 2019-2021 Aleo Systems Inc.
// This file is part of the snarkOS library.

// The snarkOS library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkOS library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkOS library. If not, see <https://www.gnu.org/licenses/>.

use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{canvas::Canvas, Block, Borders},
    Frame,
};

pub(super) struct Logs;

impl Logs {
    pub(super) fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        // Initialize the layout of the page.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(100)].as_ref())
            .split(area);

        let canvas = Canvas::default()
            .block(Block::default().borders(Borders::ALL).title("Logs"))
            .paint(|_ctx| {
                // ctx.draw(&ball);
            });
        f.render_widget(canvas, chunks[0]);
    }
}
