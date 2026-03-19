//! Renders a window's UI tree into iced `Element`s via toddy-core's widget
//! mapper.

use iced::widget::{container, text};
use iced::{Element, Fill, window};

use toddy_core::message::Message;

use super::App;

impl App {
    /// Render a single window's UI tree into iced `Element`s.
    /// Called by the iced daemon for each open window on every frame.
    pub(super) fn view_window(&self, window_id: window::Id) -> Element<'_, Message> {
        let toddy_id = match self.windows.get_toddy(&window_id) {
            Some(id) => id,
            None => {
                return container(text("unknown window"))
                    .width(Fill)
                    .height(Fill)
                    .center(Fill)
                    .into();
            }
        };

        let resolved_theme = self.theme_ref_for_window(window_id);

        match self.core.tree.find_window(toddy_id) {
            Some(window_node) => {
                let ctx = toddy_core::extensions::RenderCtx {
                    caches: &self.core.caches,
                    images: &self.image_registry,
                    theme: resolved_theme,
                    extensions: &self.dispatcher,
                    default_text_size: self.core.default_text_size,
                    default_font: self.core.default_font,
                };
                toddy_core::widgets::render(window_node, ctx)
            }
            None => container(text("waiting for snapshot..."))
                .width(Fill)
                .height(Fill)
                .center(Fill)
                .into(),
        }
    }
}
