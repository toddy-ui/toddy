use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use iced::Font;
use iced::widget::canvas as iced_canvas;
use iced::widget::{combo_box, markdown, pane_grid, text_editor};
use serde_json::Value;

use super::helpers::*;
use crate::protocol::TreeNode;

/// Maximum recursion depth for tree walks (render, ensure_caches, prepare).
/// Prevents stack overflow from pathologically nested trees. Normal UI trees
/// rarely exceed 20-30 levels; 256 is generous.
pub(crate) const MAX_TREE_DEPTH: usize = 256;

// ---------------------------------------------------------------------------
// Widget caches
// ---------------------------------------------------------------------------

/// Bundles all per-widget caches into a single struct so render functions
/// don't need to thread 3+ separate HashMap parameters everywhere.
pub struct WidgetCaches {
    pub(crate) editor_contents: HashMap<String, text_editor::Content>,
    /// Tracks the hash of the last-synced "content" prop for each text_editor.
    /// Used to detect host-side prop changes without clobbering user edits.
    pub(crate) editor_content_hashes: HashMap<String, u64>,
    pub(crate) markdown_items: HashMap<String, (u64, Vec<markdown::Item>)>,
    pub(crate) combo_states: HashMap<String, combo_box::State<String>>,
    pub(crate) combo_options: HashMap<String, Vec<String>>,
    pub(crate) pane_grid_states: HashMap<String, pane_grid::State<String>>,
    /// Per-canvas, per-layer geometry caches. Outer key is node ID, inner key
    /// is layer name. The u64 is a content hash of the layer's shapes array --
    /// when it changes the cache is cleared so the layer re-tessellates.
    pub(crate) canvas_caches: HashMap<String, HashMap<String, (u64, iced_canvas::Cache)>>,
    /// Per-qr_code caches. Key is node ID, value is (content hash, canvas Cache).
    pub(crate) qr_code_caches: HashMap<String, (u64, iced_canvas::Cache)>,
    /// Resolved themes for Themer widget nodes. Populated in ensure_caches()
    /// so render_themer() can borrow them with the correct lifetime.
    pub(crate) themer_themes: HashMap<String, iced::Theme>,
    pub(crate) default_text_size: Option<f32>,
    pub(crate) default_font: Option<Font>,
    pub extension: crate::extensions::ExtensionCaches,
}

impl Default for WidgetCaches {
    fn default() -> Self {
        Self::new()
    }
}

impl WidgetCaches {
    pub fn new() -> Self {
        Self {
            editor_contents: HashMap::new(),
            editor_content_hashes: HashMap::new(),
            markdown_items: HashMap::new(),
            combo_states: HashMap::new(),
            combo_options: HashMap::new(),
            pane_grid_states: HashMap::new(),
            canvas_caches: HashMap::new(),
            qr_code_caches: HashMap::new(),
            themer_themes: HashMap::new(),
            default_text_size: None,
            default_font: None,
            extension: crate::extensions::ExtensionCaches::new(),
        }
    }

    pub fn clear(&mut self) {
        self.clear_builtin();
        self.extension.clear();
    }

    // -- Accessor methods for renderer crate --
    // Fields are pub(crate) to avoid leaking internal HashMap structure to
    // extension authors, but the renderer binary needs access to a few.

    /// Get a mutable reference to a text_editor Content by node ID.
    pub fn editor_content_mut(&mut self, id: &str) -> Option<&mut text_editor::Content> {
        self.editor_contents.get_mut(id)
    }

    /// Get a mutable reference to a pane_grid State by node ID.
    pub fn pane_grid_state_mut(&mut self, id: &str) -> Option<&mut pane_grid::State<String>> {
        self.pane_grid_states.get_mut(id)
    }

    /// Get an immutable reference to a pane_grid State by node ID.
    pub fn pane_grid_state(&self, id: &str) -> Option<&pane_grid::State<String>> {
        self.pane_grid_states.get(id)
    }

    /// Clear built-in widget caches without touching extension caches.
    ///
    /// Used by the Snapshot handler so that extension cleanup callbacks
    /// (via `ExtensionDispatcher::prepare_all`) can run before the
    /// extension cache entries are removed.
    pub fn clear_builtin(&mut self) {
        self.editor_contents.clear();
        self.editor_content_hashes.clear();
        self.markdown_items.clear();
        self.combo_states.clear();
        self.combo_options.clear();
        self.pane_grid_states.clear();
        self.canvas_caches.clear();
        self.qr_code_caches.clear();
        self.themer_themes.clear();
    }
}

// ---------------------------------------------------------------------------
// Cache pre-population
// ---------------------------------------------------------------------------

/// Walk the tree and ensure that every `text_editor`, `markdown`,
/// `combo_box`, `pane_grid`, `canvas`, and `qr_code` node has an entry in
/// the corresponding cache. This must be called *before* `render` so that
/// `render` can work with shared (`&`) references to the caches.
///
/// After populating caches, prunes stale entries for nodes no longer in the
/// tree across all cache types.
pub fn ensure_caches(node: &TreeNode, caches: &mut WidgetCaches) {
    let mut live_ids = HashSet::new();
    ensure_caches_walk(node, caches, &mut live_ids, 0);
    prune_all_stale_caches(&live_ids, caches);
}

/// Inner recursive walk: populate caches and collect live node IDs.
fn ensure_caches_walk(
    node: &TreeNode,
    caches: &mut WidgetCaches,
    live_ids: &mut HashSet<String>,
    depth: usize,
) {
    if depth > MAX_TREE_DEPTH {
        log::warn!(
            "[id={}] ensure_caches depth exceeds {MAX_TREE_DEPTH}, skipping subtree",
            node.id
        );
        return;
    }
    live_ids.insert(node.id.clone());

    match node.type_name.as_str() {
        "text_editor" => {
            let props = node.props.as_object();
            let content_str = prop_str(props, "content").unwrap_or_default();
            let prop_hash = hash_str(&content_str);
            let prev_hash = caches.editor_content_hashes.get(&node.id).copied();
            if prev_hash != Some(prop_hash) {
                // Host changed the content prop -- (re)create the Content.
                caches.editor_contents.insert(
                    node.id.clone(),
                    text_editor::Content::with_text(&content_str),
                );
                caches
                    .editor_content_hashes
                    .insert(node.id.clone(), prop_hash);
            }
            // If hash matches, Content is already initialized and we preserve
            // any user edits that happened since the last prop sync.
        }
        "markdown" => {
            let props = node.props.as_object();
            let content_str = prop_str(props, "content").unwrap_or_default();
            let code_theme_str = prop_str(props, "code_theme").unwrap_or_default();
            let hash = hash_str(&format!("{content_str}\0{code_theme_str}"));
            match caches.markdown_items.get(&node.id) {
                Some((existing_hash, _)) if *existing_hash == hash => {}
                _ => {
                    let code_theme = match code_theme_str.as_str() {
                        "base16_mocha" => Some(iced::highlighter::Theme::Base16Mocha),
                        "base16_ocean" => Some(iced::highlighter::Theme::Base16Ocean),
                        "base16_eighties" => Some(iced::highlighter::Theme::Base16Eighties),
                        "solarized_dark" => Some(iced::highlighter::Theme::SolarizedDark),
                        "inspired_github" => Some(iced::highlighter::Theme::InspiredGitHub),
                        "" => None,
                        other => {
                            log::warn!("unknown code_theme {:?}, using default", other);
                            None
                        }
                    };
                    let items: Vec<_> = if let Some(theme) = code_theme {
                        let mut md = markdown::Content::new().code_theme(theme);
                        md.push_str(&content_str);
                        md.items().to_vec()
                    } else {
                        markdown::parse(&content_str).collect()
                    };
                    caches.markdown_items.insert(node.id.clone(), (hash, items));
                }
            }
        }
        "combo_box" => {
            let props = node.props.as_object();
            let options: Vec<String> = props
                .and_then(|p| p.get("options"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let cached_options = caches.combo_options.get(&node.id);
            let options_changed = cached_options.is_none_or(|cached| *cached != options);
            if options_changed {
                caches
                    .combo_states
                    .insert(node.id.clone(), combo_box::State::new(options.clone()));
                caches.combo_options.insert(node.id.clone(), options);
            }
        }
        "pane_grid" => {
            let props = node.props.as_object();
            let axis = match prop_str(props, "split_axis").as_deref() {
                Some("horizontal") => pane_grid::Axis::Horizontal,
                _ => pane_grid::Axis::Vertical,
            };
            let child_ids: HashSet<String> = node.children.iter().map(|c| c.id.clone()).collect();

            if let Some(state) = caches.pane_grid_states.get_mut(&node.id) {
                // Prune panes whose child nodes no longer exist.
                let stale_panes: Vec<pane_grid::Pane> = state
                    .panes
                    .iter()
                    .filter(|(_pane, id)| !child_ids.contains(*id))
                    .map(|(pane, _id)| *pane)
                    .collect();
                for pane in stale_panes {
                    state.close(pane);
                }
                // Add panes for new children that don't have a pane yet.
                // Collect owned IDs to avoid holding an immutable borrow on
                // state.panes while we call state.split() (mutable).
                let existing_ids: HashSet<String> = state.panes.values().cloned().collect();
                let new_child_ids: Vec<String> = node
                    .children
                    .iter()
                    .filter(|c| !existing_ids.contains(&c.id))
                    .map(|c| c.id.clone())
                    .collect();
                for new_id in new_child_ids {
                    if let Some((&anchor, _)) = state.panes.iter().next() {
                        let _ = state.split(axis, anchor, new_id);
                    }
                }
            } else {
                let child_list: Vec<String> = node.children.iter().map(|c| c.id.clone()).collect();
                let new_state = if child_list.is_empty() {
                    let (state, _) = pane_grid::State::new("default".to_string());
                    state
                } else if child_list.len() == 1 {
                    let (state, _) = pane_grid::State::new(child_list[0].clone());
                    state
                } else {
                    let (mut state, first_pane) = pane_grid::State::new(child_list[0].clone());
                    let mut last_pane = first_pane;
                    for id in child_list.iter().skip(1) {
                        if let Some((new_pane, _)) = state.split(axis, last_pane, id.clone()) {
                            last_pane = new_pane;
                        }
                    }
                    state
                };
                caches.pane_grid_states.insert(node.id.clone(), new_state);
            }
        }
        "canvas" => {
            let props = node.props.as_object();
            // Build layer map: either from "layers" (object) or "shapes" (array -> single layer).
            let layer_map = canvas_layer_map(props);
            let node_caches = caches.canvas_caches.entry(node.id.clone()).or_default();

            // Update or create caches for each layer.
            for (layer_name, shapes_val) in &layer_map {
                let hash = {
                    let mut hasher = DefaultHasher::new();
                    hash_json_value(shapes_val, &mut hasher);
                    hasher.finish()
                };
                match node_caches.get_mut(layer_name) {
                    Some((existing_hash, cache)) => {
                        if *existing_hash != hash {
                            cache.clear();
                            // Update just the hash, keep the same cache object.
                            *existing_hash = hash;
                        }
                    }
                    None => {
                        node_caches.insert(layer_name.clone(), (hash, iced_canvas::Cache::new()));
                    }
                }
            }

            // Remove stale layers that are no longer in the tree.
            node_caches.retain(|name, _| layer_map.contains_key(name));
        }
        "themer" => {
            let props = node.props.as_object();
            if let Some(resolved) = props
                .and_then(|p| p.get("theme"))
                .and_then(crate::theming::resolve_theme_only)
            {
                caches.themer_themes.insert(node.id.clone(), resolved);
            } else {
                // No valid theme prop -- remove stale cache entry if present.
                caches.themer_themes.remove(&node.id);
            }
        }
        "qr_code" => {
            let props = node.props.as_object();
            let data = prop_str(props, "data").unwrap_or_default();
            let cell_size = prop_f32(props, "cell_size").unwrap_or(4.0);
            let ec = prop_str(props, "error_correction").unwrap_or_default();
            // Hash data + cell_size + error_correction for cache invalidation.
            let mut hasher = DefaultHasher::new();
            data.hash(&mut hasher);
            cell_size.to_bits().hash(&mut hasher);
            ec.hash(&mut hasher);
            let hash = hasher.finish();

            match caches.qr_code_caches.get_mut(&node.id) {
                Some((existing_hash, cache)) => {
                    if *existing_hash != hash {
                        cache.clear();
                        // Update just the hash, keep the same cache object.
                        *existing_hash = hash;
                    }
                }
                None => {
                    caches
                        .qr_code_caches
                        .insert(node.id.clone(), (hash, iced_canvas::Cache::new()));
                }
            }
        }
        _ => {}
    }

    for child in &node.children {
        ensure_caches_walk(child, caches, live_ids, depth + 1);
    }
}

/// Prune all cache types, removing entries whose node IDs are no longer live.
fn prune_all_stale_caches(live_ids: &HashSet<String>, caches: &mut WidgetCaches) {
    caches.editor_contents.retain(|id, _| live_ids.contains(id));
    caches
        .editor_content_hashes
        .retain(|id, _| live_ids.contains(id));
    caches.markdown_items.retain(|id, _| live_ids.contains(id));
    caches.combo_states.retain(|id, _| live_ids.contains(id));
    caches.combo_options.retain(|id, _| live_ids.contains(id));
    caches
        .pane_grid_states
        .retain(|id, _| live_ids.contains(id));
    caches.canvas_caches.retain(|id, _| live_ids.contains(id));
    caches.qr_code_caches.retain(|id, _| live_ids.contains(id));
    caches.themer_themes.retain(|id, _| live_ids.contains(id));
}

// ---------------------------------------------------------------------------
// Canvas cache helpers (used by ensure_caches)
// ---------------------------------------------------------------------------

/// Build a sorted layer map from canvas props. Supports two prop formats:
/// - `"layers"`: a JSON object mapping layer_name -> array of shapes (preferred)
/// - `"shapes"`: a flat JSON array of shapes (legacy, wrapped as a single "default" layer)
///
/// If both are present, `"layers"` wins. Returns a BTreeMap of references so
/// layer order is deterministic (alphabetical by name) without allocating
/// serialized strings.
pub(crate) fn canvas_layer_map(
    props: Option<&serde_json::Map<String, Value>>,
) -> std::collections::BTreeMap<String, &Value> {
    let mut map = std::collections::BTreeMap::new();

    if let Some(layers_obj) = props
        .and_then(|p| p.get("layers"))
        .and_then(|v| v.as_object())
    {
        for (name, shapes_val) in layers_obj {
            map.insert(name.clone(), shapes_val);
        }
    } else if let Some(shapes_arr) = props.and_then(|p| p.get("shapes")) {
        map.insert("default".to_string(), shapes_arr);
    }

    map
}

/// Hash a serde_json::Value recursively without allocating a serialized string.
/// Each variant is discriminated by a type tag byte to avoid collisions.
///
/// NOTE: DefaultHasher output is not stable across Rust versions or builds.
/// These hashes must never be persisted or compared across process restarts.
pub(crate) fn hash_json_value(v: &serde_json::Value, h: &mut impl std::hash::Hasher) {
    match v {
        serde_json::Value::Null => 0u8.hash(h),
        serde_json::Value::Bool(b) => {
            1u8.hash(h);
            b.hash(h);
        }
        serde_json::Value::Number(n) => {
            2u8.hash(h);
            if let Some(f) = n.as_f64() {
                f.to_bits().hash(h);
            } else if let Some(i) = n.as_i64() {
                i.hash(h);
            } else if let Some(u) = n.as_u64() {
                u.hash(h);
            }
        }
        serde_json::Value::String(s) => {
            3u8.hash(h);
            s.hash(h);
        }
        serde_json::Value::Array(arr) => {
            4u8.hash(h);
            arr.len().hash(h);
            for item in arr {
                hash_json_value(item, h);
            }
        }
        serde_json::Value::Object(obj) => {
            5u8.hash(h);
            obj.len().hash(h);
            for (k, v) in obj {
                k.hash(h);
                hash_json_value(v, h);
            }
        }
    }
}

/// Hash a string using DefaultHasher for same-process cache invalidation.
/// NOTE: DefaultHasher output is not stable across Rust versions or builds.
/// These hashes must never be persisted or compared across process restarts.
pub(crate) fn hash_str(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- WidgetCaches --

    #[test]
    fn widget_caches_new_is_empty() {
        let c = WidgetCaches::new();
        assert!(c.editor_contents.is_empty());
        assert!(c.markdown_items.is_empty());
        assert!(c.combo_states.is_empty());
        assert!(c.combo_options.is_empty());
        assert!(c.pane_grid_states.is_empty());
        assert!(c.default_text_size.is_none());
        assert!(c.default_font.is_none());
    }

    #[test]
    fn widget_caches_clear_empties_maps_but_preserves_defaults() {
        let mut c = WidgetCaches::new();
        c.default_text_size = Some(14.0);
        c.default_font = Some(Font::MONOSPACE);
        c.combo_options.insert("x".into(), vec!["a".into()]);
        c.clear();
        assert!(c.combo_options.is_empty());
        assert_eq!(c.default_text_size, Some(14.0));
        assert_eq!(c.default_font, Some(Font::MONOSPACE));
    }

    // -- clear_builtin vs clear --

    #[test]
    fn clear_builtin_preserves_extension_caches() {
        let mut caches = WidgetCaches::new();

        // Add a built-in cache entry and an extension cache entry.
        caches
            .editor_contents
            .insert("ed1".to_string(), iced::widget::text_editor::Content::new());
        caches.extension.insert("ext", "key", 42u32);

        caches.clear_builtin();

        // Built-in caches should be empty.
        assert!(caches.editor_contents.is_empty());
        // Extension caches should survive.
        assert_eq!(caches.extension.get::<u32>("ext", "key"), Some(&42));
    }

    #[test]
    fn clear_wipes_both_builtin_and_extension() {
        let mut caches = WidgetCaches::new();

        caches
            .editor_contents
            .insert("ed1".to_string(), iced::widget::text_editor::Content::new());
        caches.extension.insert("ext", "key", 42u32);

        caches.clear();

        assert!(caches.editor_contents.is_empty());
        assert!(!caches.extension.contains("ext", "key"));
    }

    // -- hash_json_value --

    #[test]
    fn hash_json_value_same_input_same_hash() {
        use std::collections::hash_map::DefaultHasher;

        let val = serde_json::json!({"shapes": [{"type": "rect", "x": 0, "y": 0}]});
        let h1 = {
            let mut h = DefaultHasher::new();
            hash_json_value(&val, &mut h);
            h.finish()
        };
        let h2 = {
            let mut h = DefaultHasher::new();
            hash_json_value(&val, &mut h);
            h.finish()
        };
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_json_value_different_input_different_hash() {
        use std::collections::hash_map::DefaultHasher;

        let a = serde_json::json!({"type": "rect"});
        let b = serde_json::json!({"type": "circle"});
        let hash_a = {
            let mut h = DefaultHasher::new();
            hash_json_value(&a, &mut h);
            h.finish()
        };
        let hash_b = {
            let mut h = DefaultHasher::new();
            hash_json_value(&b, &mut h);
            h.finish()
        };
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn hash_json_value_type_discrimination() {
        use std::collections::hash_map::DefaultHasher;

        // null, false, and 0 should produce different hashes
        let vals = [
            serde_json::json!(null),
            serde_json::json!(false),
            serde_json::json!(0),
            serde_json::json!(""),
            serde_json::json!([]),
            serde_json::json!({}),
        ];
        let hashes: Vec<u64> = vals
            .iter()
            .map(|v| {
                let mut h = DefaultHasher::new();
                hash_json_value(v, &mut h);
                h.finish()
            })
            .collect();

        // All should be distinct
        for (i, h1) in hashes.iter().enumerate() {
            for (j, h2) in hashes.iter().enumerate() {
                if i != j {
                    assert_ne!(h1, h2, "type {i} and {j} should hash differently");
                }
            }
        }
    }
}
