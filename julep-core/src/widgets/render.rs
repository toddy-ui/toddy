use std::cell::Cell;

use iced::widget::{Space, container, text};
use iced::{Color, Element};

use super::caches::{MAX_TREE_DEPTH, WidgetCaches};
use super::helpers::*;
use super::{canvas, display, input, interactive, layout, table, validate};
use crate::extensions::ExtensionDispatcher;
use crate::message::Message;
use crate::protocol::TreeNode;

// ---------------------------------------------------------------------------
// Main render dispatch
// ---------------------------------------------------------------------------

/// Map a TreeNode to an iced Element. Unknown types render as an empty container.
///
/// This is the immutable side of the ensure_caches/render split. All mutable
/// cache state (text_editor Content, markdown Items, combo_box State, canvas
/// Cache, etc.) must be pre-populated by [`super::ensure_caches`] before calling
/// this function. `render` works exclusively with shared (`&`) references
/// to caches, so it can run inside iced's `view()` which only has `&self`.
pub fn render<'a>(
    node: &'a TreeNode,
    caches: &'a WidgetCaches,
    images: &'a crate::image_registry::ImageRegistry,
    theme: &'a iced::Theme,
    dispatcher: &'a ExtensionDispatcher,
) -> Element<'a, Message> {
    // Track recursion depth via thread-local counter. Each call increments
    // on entry; the DepthGuard decrements on drop (including early returns).
    thread_local! {
        static RENDER_DEPTH: Cell<usize> = const { Cell::new(0) };
    }
    struct DepthGuard;
    impl Drop for DepthGuard {
        fn drop(&mut self) {
            RENDER_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }

    let depth = RENDER_DEPTH.with(|d| {
        let new = d.get() + 1;
        d.set(new);
        new
    });
    let _guard = DepthGuard;

    if depth > MAX_TREE_DEPTH {
        log::warn!(
            "[id={}] render depth exceeds {MAX_TREE_DEPTH}, returning placeholder",
            node.id
        );
        return text("Max depth exceeded")
            .color(Color::from_rgb(1.0, 0.0, 0.0))
            .into();
    }

    if validate::is_validate_props_enabled() {
        validate::validate_props(node);
    }

    let element = match node.type_name.as_str() {
        // Layout widgets
        "column" => layout::render_column(node, caches, images, theme, dispatcher),
        "row" => layout::render_row(node, caches, images, theme, dispatcher),
        "container" => layout::render_container(node, caches, images, theme, dispatcher),
        "stack" => layout::render_stack(node, caches, images, theme, dispatcher),
        "grid" => layout::render_grid(node, caches, images, theme, dispatcher),
        "pin" => layout::render_pin(node, caches, images, theme, dispatcher),
        "keyed_column" => layout::render_keyed_column(node, caches, images, theme, dispatcher),
        "float" => layout::render_float(node, caches, images, theme, dispatcher),
        "responsive" => layout::render_responsive(node, caches, images, theme, dispatcher),
        "scrollable" => layout::render_scrollable(node, caches, images, theme, dispatcher),
        "pane_grid" => layout::render_pane_grid(node, caches, images, theme, dispatcher),
        // Display widgets
        "text" => display::render_text(node, caches),
        "rich_text" | "rich" => display::render_rich_text(node, caches),
        "space" => display::render_space(node),
        "rule" => display::render_rule(node),
        "progress_bar" => display::render_progress_bar(node),
        "image" => display::render_image(node, images),
        "svg" => display::render_svg(node),
        "markdown" => display::render_markdown(node, caches, theme),
        "qr_code" => display::render_qr_code(node, caches),
        // Input widgets
        "text_input" => input::render_text_input(node, caches),
        "text_editor" => input::render_text_editor(node, caches),
        "checkbox" => input::render_checkbox(node, caches),
        "toggler" => input::render_toggler(node, caches),
        "radio" => input::render_radio(node, caches),
        "slider" => input::render_slider(node),
        "vertical_slider" => input::render_vertical_slider(node),
        "pick_list" => input::render_pick_list(node, caches),
        "combo_box" => input::render_combo_box(node, caches),
        // Interactive widgets
        "button" => interactive::render_button(node, caches, images, theme, dispatcher),
        "mouse_area" => interactive::render_mouse_area(node, caches, images, theme, dispatcher),
        "sensor" => interactive::render_sensor(node, caches, images, theme, dispatcher),
        "tooltip" => interactive::render_tooltip(node, caches, images, theme, dispatcher),
        "themer" => interactive::render_themer(node, caches, images, theme, dispatcher),
        "window" => interactive::render_window(node, caches, images, theme, dispatcher),
        "overlay" => interactive::render_overlay(node, caches, images, theme, dispatcher),
        // Canvas
        "canvas" => canvas::render_canvas(node, caches, images, theme, dispatcher),
        // Table
        "table" => table::render_table(node),
        // Extension dispatch
        unknown => {
            if dispatcher.handles_type(unknown) {
                let render_ctx = crate::extensions::RenderContext {
                    caches,
                    images,
                    theme,
                    extensions: dispatcher,
                };
                let env = crate::extensions::WidgetEnv {
                    caches: &caches.extension,
                    images,
                    theme,
                    render_ctx,
                    default_text_size: caches.default_text_size,
                    default_font: caches.default_font,
                };
                // catch_unwind at the render boundary: extension panics produce
                // a red placeholder instead of crashing the renderer.
                // We track consecutive render panics via an atomic counter
                // on the dispatcher; after N consecutive panics, the
                // extension is poisoned on the next prepare_all cycle.
                if crate::extensions::catch_unwind_enabled() {
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dispatcher.render(node, &env)
                    })) {
                        Ok(Some(element)) => element,
                        Ok(None) => container(Space::new()).into(),
                        Err(_) => {
                            let at_threshold = dispatcher.record_render_panic(unknown);
                            if at_threshold {
                                log::error!(
                                    "[id={}] extension for type `{unknown}` hit render panic \
                                     threshold, will be poisoned on next prepare cycle",
                                    node.id
                                );
                            } else {
                                log::error!("extension panicked in render for node `{}`", node.id);
                            }
                            iced::widget::text(format!(
                                "Extension error: type `{unknown}`, node `{}`",
                                node.id
                            ))
                            .color(iced::Color::from_rgb(1.0, 0.0, 0.0))
                            .into()
                        }
                    }
                } else {
                    match dispatcher.render(node, &env) {
                        Some(element) => element,
                        None => container(Space::new()).into(),
                    }
                }
            } else {
                log::warn!(
                    "[id={}] unknown node type `{unknown}`, rendering as empty container",
                    node.id
                );
                container(Space::new()).into()
            }
        }
    };

    // Explicit a11y overrides take precedence.
    let overrides = crate::a11y_widget::A11yOverrides::from_props(&node.props).or_else(|| {
        // Auto-infer accessibility overrides from widget-specific props
        // when the host hasn't set an explicit a11y block.
        let props = node.props.as_object();
        match node.type_name.as_str() {
            // Image and SVG use iced's native .alt()/.description() methods
            // directly, so no A11yOverride wrapping needed for those.
            "text_input" | "text_editor" | "combo_box" => prop_str(props, "placeholder")
                .map(crate::a11y_widget::A11yOverrides::with_description),
            _ => None,
        }
    });

    if let Some(overrides) = overrides {
        return crate::a11y_widget::A11yOverride::wrap(element, overrides).into();
    }

    element
}

// ---------------------------------------------------------------------------
// Child rendering helper
// ---------------------------------------------------------------------------

pub(crate) fn render_children<'a>(
    node: &'a TreeNode,
    caches: &'a WidgetCaches,
    images: &'a crate::image_registry::ImageRegistry,
    theme: &'a iced::Theme,
    dispatcher: &'a ExtensionDispatcher,
) -> Vec<Element<'a, Message>> {
    node.children
        .iter()
        .map(|c| render(c, caches, images, theme, dispatcher))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::ExtensionDispatcher;
    use crate::image_registry::ImageRegistry;
    use crate::protocol::TreeNode;

    // -- Image registry handle lookup --

    #[test]
    fn image_registry_handle_lookup() {
        let mut registry = ImageRegistry::new();
        // Minimal valid 1x1 RGB PNG.
        let png_bytes: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
            0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // 8-bit RGB
            0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, // IDAT
            0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE2,
            0x21, 0xBC, 0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, // IEND
            0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        registry
            .create_from_bytes("test_sprite", png_bytes)
            .expect("test sprite should be valid");
        assert!(
            registry.get("test_sprite").is_some(),
            "registered handle should be retrievable"
        );
        assert!(
            registry.get("nonexistent").is_none(),
            "unregistered name should return None"
        );
    }

    // -----------------------------------------------------------------------
    // Render smoke tests -- verify render() doesn't panic for common types
    // -----------------------------------------------------------------------

    fn smoke_node(id: &str, type_name: &str, props: serde_json::Value) -> TreeNode {
        TreeNode {
            id: id.to_string(),
            type_name: type_name.to_string(),
            props,
            children: vec![],
        }
    }

    fn smoke_node_with_children(
        id: &str,
        type_name: &str,
        props: serde_json::Value,
        children: Vec<TreeNode>,
    ) -> TreeNode {
        TreeNode {
            id: id.to_string(),
            type_name: type_name.to_string(),
            props,
            children,
        }
    }

    fn smoke_text_child() -> TreeNode {
        smoke_node("child", "text", serde_json::json!({"content": "hi"}))
    }

    #[test]
    fn render_smoke_text() {
        let node = smoke_node("t", "text", serde_json::json!({"content": "hello"}));
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_column_empty() {
        let node = smoke_node("c", "column", serde_json::json!({}));
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_row_empty() {
        let node = smoke_node("r", "row", serde_json::json!({}));
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_container_with_child() {
        let node = smoke_node_with_children(
            "ct",
            "container",
            serde_json::json!({}),
            vec![smoke_text_child()],
        );
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_button_with_child() {
        let node = smoke_node_with_children(
            "btn",
            "button",
            serde_json::json!({}),
            vec![smoke_text_child()],
        );
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_checkbox() {
        let node = smoke_node(
            "cb",
            "checkbox",
            serde_json::json!({"label": "Accept", "checked": true}),
        );
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_space() {
        let node = smoke_node("sp", "space", serde_json::json!({}));
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_rule() {
        let node = smoke_node("rl", "rule", serde_json::json!({"direction": "horizontal"}));
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_progress_bar() {
        let node = smoke_node(
            "pb",
            "progress_bar",
            serde_json::json!({"value": 50.0, "min": 0.0, "max": 100.0}),
        );
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_slider() {
        let node = smoke_node(
            "sl",
            "slider",
            serde_json::json!({"min": 0.0, "max": 100.0, "value": 50.0}),
        );
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_text_input() {
        let node = smoke_node(
            "ti",
            "text_input",
            serde_json::json!({"placeholder": "Type here", "value": ""}),
        );
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_toggler() {
        let node = smoke_node("tg", "toggler", serde_json::json!({"is_toggled": false}));
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_smoke_stack_empty() {
        let node = smoke_node("st", "stack", serde_json::json!({}));
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    // -----------------------------------------------------------------------
    // Error path tests -- unknown type and missing props
    // -----------------------------------------------------------------------

    #[test]
    fn render_unknown_type_returns_element_without_panic() {
        let node = smoke_node("unk", "definitely_not_a_widget", serde_json::json!({}));
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        // Should produce the empty container fallback, not panic.
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    #[test]
    fn render_text_input_missing_props_does_not_panic() {
        let node = smoke_node("ti_empty", "text_input", serde_json::json!({}));
        let caches = WidgetCaches::new();
        let images = ImageRegistry::new();
        let theme = iced::Theme::Dark;
        let dispatcher = ExtensionDispatcher::default();
        let _elem = render(&node, &caches, &images, &theme, &dispatcher);
    }

    // -----------------------------------------------------------------------
    // A11y auto-inference tests
    // -----------------------------------------------------------------------

    /// Helper: extract auto-inferred overrides the same way render() does,
    /// without actually rendering (avoids needing image handles etc.).
    fn infer_a11y_overrides(node: &TreeNode) -> Option<crate::a11y_widget::A11yOverrides> {
        crate::a11y_widget::A11yOverrides::from_props(&node.props).or_else(|| {
            let props = node.props.as_object();
            match node.type_name.as_str() {
                // Image and SVG use iced's native .alt()/.description() methods
                // directly, so no A11yOverride wrapping needed for those.
                "text_input" | "text_editor" | "combo_box" => prop_str(props, "placeholder")
                    .map(crate::a11y_widget::A11yOverrides::with_description),
                _ => None,
            }
        })
    }

    #[test]
    fn a11y_image_alt_uses_native_iced_method_not_override() {
        // Image/SVG alt text is handled by iced's native .alt() method,
        // not by A11yOverride wrapping. No override should be created.
        let node = smoke_node(
            "img1",
            "image",
            serde_json::json!({"source": "logo.png", "alt": "Company logo"}),
        );
        assert!(
            infer_a11y_overrides(&node).is_none(),
            "image with alt should NOT get A11yOverride (uses native .alt())"
        );
    }

    #[test]
    fn a11y_svg_alt_uses_native_iced_method_not_override() {
        let node = smoke_node(
            "svg1",
            "svg",
            serde_json::json!({"source": "icon.svg", "alt": "Settings icon"}),
        );
        assert!(
            infer_a11y_overrides(&node).is_none(),
            "svg with alt should NOT get A11yOverride (uses native .alt())"
        );
    }

    #[test]
    fn a11y_auto_infer_text_input_placeholder_as_description() {
        let node = smoke_node(
            "ti1",
            "text_input",
            serde_json::json!({"placeholder": "Search...", "value": ""}),
        );
        let overrides =
            infer_a11y_overrides(&node).expect("should infer overrides from placeholder");
        assert_eq!(overrides.description.as_deref(), Some("Search..."));
        assert!(overrides.label.is_none());
    }

    #[test]
    fn a11y_explicit_overrides_take_precedence_over_alt() {
        let node = smoke_node(
            "img2",
            "image",
            serde_json::json!({
                "source": "logo.png",
                "alt": "Auto alt",
                "a11y": {"label": "Explicit label"}
            }),
        );
        let overrides = infer_a11y_overrides(&node).expect("should have explicit overrides");
        // Explicit label wins; no double-wrapping.
        assert_eq!(overrides.label.as_deref(), Some("Explicit label"));
    }

    #[test]
    fn a11y_no_wrapping_without_alt_or_a11y() {
        let node = smoke_node("txt1", "text", serde_json::json!({"content": "hello"}));
        assert!(
            infer_a11y_overrides(&node).is_none(),
            "plain text node should not get a11y wrapping"
        );
    }

    #[test]
    fn a11y_no_wrapping_image_without_alt() {
        let node = smoke_node(
            "img3",
            "image",
            serde_json::json!({"source": "decorative.png"}),
        );
        assert!(
            infer_a11y_overrides(&node).is_none(),
            "image without alt should not get a11y wrapping"
        );
    }

    #[test]
    fn a11y_no_wrapping_text_input_without_placeholder() {
        let node = smoke_node(
            "ti2",
            "text_input",
            serde_json::json!({"value": "typed text"}),
        );
        assert!(
            infer_a11y_overrides(&node).is_none(),
            "text_input without placeholder should not get a11y wrapping"
        );
    }
}
