//! Project save / load persistence: building a `bar_project::Project`
//! from the live editor state, saving to disk, and the inverse of
//! re-installing absolute paths after a load. Distributed
//! `impl BarEditorApp` block.
//!
//! The save flow is two-step: first `pack_assets_for_save` copies
//! external assets into `<stem>.assets/` and rewrites params to
//! `bar://...` URLs, then `build_project` snapshots the live state
//! into a `Project` and `save_project` serialises and writes.

use std::collections::HashMap;

use bar_graph::{NodeId, NodeType};

use crate::app::{BarEditorApp, CanvasView};
use crate::project::path::{
    pack_passthrough_files, pack_path_param, resolve_passthrough_files, resolve_path_param,
};
use crate::t;

impl BarEditorApp {
    /// Snapshot the live editor state into a `bar_project::Project`
    /// suitable for serialisation. Called by both the user-initiated
    /// save flow and the autosave path.
    pub(crate) fn build_project(&mut self, path: &std::path::Path) -> bar_project::Project {
        use bar_project::recipe::{
            MapSettings, OutputConfig, Recipe, RecipeConnection, RecipeNode,
        };
        use bar_project::{EditorLayout, Position, Project};

        let mut nodes = Vec::new();
        let mut key_map: HashMap<NodeId, String> = HashMap::new();
        let mut layout_positions: HashMap<String, Position> = HashMap::new();
        let mut layout_sizes: HashMap<String, bar_project::NodeSize> = HashMap::new();

        for (id, node) in self.graph.nodes() {
            let key = format!("node_{}", id.0);
            key_map.insert(*id, key.clone());
            nodes.push(RecipeNode {
                key: key.clone(),
                node_type: node.node_type.clone(),
                label: node.label.clone(),
                params: node.params.clone(),
            });

            if let Some(visual) = self.visuals.node_visuals.get(id) {
                layout_positions.insert(
                    key.clone(),
                    Position {
                        x: visual.position.x,
                        y: visual.position.y,
                    },
                );
                layout_sizes.insert(
                    key,
                    bar_project::NodeSize {
                        width: visual.size.x,
                        height: visual.size.y,
                    },
                );
            }
        }

        let connections = self
            .graph
            .connections()
            .iter()
            .filter_map(|conn| {
                let from_key = key_map.get(&conn.from.node_id)?;
                let to_key = key_map.get(&conn.to.node_id)?;
                Some(RecipeConnection {
                    from: format!("{}.{}", from_key, conn.from.port_name),
                    to: format!("{}.{}", to_key, conn.to.port_name),
                })
            })
            .collect();

        let recipe = Recipe {
            schema_version: bar_project::RECIPE_SCHEMA_VERSION,
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".to_string()),
            shortname: self.map.recipe_meta.shortname.clone(),
            description: self.map.recipe_meta.description.clone(),
            author: self.map.recipe_meta.author.clone(),
            version: self.map.recipe_meta.version.clone(),
            nodes,
            connections,
            output: OutputConfig {
                width: self.map.width,
                height: self.map.height,
                map_settings: MapSettings {
                    min_height: self.map.min_height,
                    max_height: self.map.max_height,
                    start_positions: self.map.settings.start_positions.clone(),
                    // Carry the structured editor's other fields
                    // (atmosphere, lighting, water, gravity, etc.) into
                    // the saved project.
                    ..self.map.settings.clone()
                },
            },
        };

        Project {
            recipe,
            layout: EditorLayout {
                node_positions: layout_positions,
                node_sizes: layout_sizes,
                canvas_offset: (self.canvas.offset.x, self.canvas.offset.y),
                map_info_file: self.project.map_info_file.clone(),
                groups: self
                    .visuals
                    .groups
                    .iter()
                    .map(|(id, g)| bar_project::NodeGroup {
                        id: *id,
                        label: g.label.clone(),
                        member_keys: g
                            .member_ids
                            .iter()
                            .filter_map(|nid| key_map.get(nid).cloned())
                            .collect(),
                        color_idx: g.color_idx,
                        collapsed: g.collapsed,
                        is_subgraph: g.is_subgraph,
                        subgraph_inputs: g
                            .subgraph_inputs
                            .iter()
                            .map(|p| bar_project::SubgraphPort {
                                name: p.name.clone(),
                                label: p.label.clone(),
                                kind: p.kind.clone(),
                                binding: p.binding.as_ref().and_then(|(nid, port_name)| {
                                    key_map.get(nid).map(|k| format!("{}:{}", k, port_name))
                                }),
                            })
                            .collect(),
                        subgraph_outputs: g
                            .subgraph_outputs
                            .iter()
                            .map(|p| bar_project::SubgraphPort {
                                name: p.name.clone(),
                                label: p.label.clone(),
                                kind: p.kind.clone(),
                                binding: p.binding.as_ref().and_then(|(nid, port_name)| {
                                    key_map.get(nid).map(|k| format!("{}:{}", k, port_name))
                                }),
                            })
                            .collect(),
                        macro_params: g
                            .macro_params
                            .iter()
                            .filter_map(|p| {
                                let (nid, param_name) = p.binding.as_ref()?;
                                let key = key_map.get(nid)?;
                                Some(bar_project::MacroParamSpec {
                                    name: p.name.clone(),
                                    label: p.label.clone(),
                                    kind: p.kind.clone(),
                                    binding: format!("{}:{}", key, param_name),
                                    min: p.min,
                                    max: p.max,
                                })
                            })
                            .collect(),
                    })
                    .collect(),
                open_tabs: self
                    .canvas
                    .tabs
                    .iter()
                    .map(|view| match view {
                        // Index 0 (Main) is implicit at load time so
                        // we don't need to write it out.
                        CanvasView::Main => bar_project::PersistedCanvasView::Main,
                        CanvasView::SubGraph(gid) => {
                            bar_project::PersistedCanvasView::SubGraph { group_id: *gid }
                        }
                    })
                    .collect(),
                active_tab: self.canvas.active_tab as u32,
            },
        }
    }

    /// User-initiated save. Pre-step: pack any referenced assets that
    /// live outside the project's own directory into
    /// `<stem>.assets/` next to the .barproj, and rewrite their paths
    /// to be project-relative. This makes saved projects portable and
    /// immune to the SD7 extract cache being pruned. Then build +
    /// serialise the project JSON, update dirty / path / recents /
    /// status.
    pub(crate) fn save_project(&mut self, path: std::path::PathBuf) {
        if let Err(e) = self.pack_assets_for_save(&path) {
            self.log_error(format!("Asset packing failed: {e}"));
            return;
        }
        let project = self.build_project(&path);
        match project.save(&path) {
            Ok(()) => {
                self.project.path = Some(path.clone());
                self.project.is_dirty = false;
                self.log_info(t!(
                    "editor.project.saved",
                    path = path.display().to_string()
                ));
                self.settings.add_recent(&path);
                self.settings.save();
                self.project.last_autosave_at = Some(std::time::Instant::now());
            }
            Err(e) => {
                self.log_error(t!("editor.project.save_failed", error = e.to_string()));
            }
        }
    }

    /// Walk every node holding a file-path param and replace any
    /// `bar://...` entries with absolute paths anchored at
    /// `project_dir`. Called after a project loads, before any
    /// evaluation, so executors always see absolute paths they can
    /// read.
    pub(crate) fn resolve_relative_paths(&mut self, project_dir: &std::path::Path) {
        for (_, node) in self.graph.nodes_mut() {
            match node.node_type {
                NodeType::FileReference => {
                    resolve_path_param(&mut node.params, "path", project_dir);
                }
                NodeType::PassThrough => {
                    resolve_passthrough_files(&mut node.params, project_dir);
                }
                _ => {}
            }
        }
    }

    /// Pack: walk every node that holds a file path; if the path
    /// lives outside the destination project's directory, copy it
    /// into `<stem>.assets/` and rewrite the param to a
    /// project-relative path. In-memory paths get rewritten too so
    /// the running session uses the new local copies (no
    /// double-evaluation needed).
    pub(crate) fn pack_assets_for_save(
        &mut self,
        project_path: &std::path::Path,
    ) -> Result<(), String> {
        let project_dir = project_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let stem = project_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("project")
            .to_string();
        let assets_dir = project_dir.join(format!("{stem}.assets"));

        // Identify which params on each node type hold an absolute
        // on-disk path that should be packed into the assets dir.
        for (_, node) in self.graph.nodes_mut() {
            match node.node_type {
                NodeType::FileReference => {
                    pack_path_param(&mut node.params, "path", &project_dir, &assets_dir, "")?;
                }
                NodeType::PassThrough => {
                    pack_passthrough_files(&mut node.params, &project_dir, &assets_dir)?;
                }
                _ => {}
            }
        }

        Ok(())
    }
}
