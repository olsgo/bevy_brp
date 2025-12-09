use std::env;
use std::fs;
use std::path::PathBuf;

use bevy_brp_mcp_macros::{ParamStruct, ResultStruct, ToolFn};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::tool::{HandlerContext, HandlerResult, ToolFn, ToolResult};

const DEFAULT_SELECTION_PATH: &str = "target/ai-selection/selection.json";
const ENV_SELECTION_PATH: &str = "BRP_GRAB_SELECTION_PATH";

fn default_true() -> bool { true }

/// Parameters for the `grab.selection` tool
#[derive(Clone, Deserialize, Serialize, JsonSchema, ParamStruct)]
pub struct GrabSelectionParams {
    /// Optional override path to the selection JSON (defaults to `target/ai-selection/selection.json` or `BRP_GRAB_SELECTION_PATH` env var)
    #[serde(default)]
    pub path: Option<String>,

    /// If true, return an error when the selection file indicates `enabled: false`
    #[serde(default)]
    pub require_enabled: bool,

    /// If true, missing file returns an error; if false, returns an empty selection result
    #[serde(default = "default_true")]
    pub fail_if_absent: bool,
}

/// Result for the `grab.selection` tool
#[derive(Debug, Clone, Serialize, Deserialize, ResultStruct)]
pub struct GrabSelectionResult {
    /// Path that was read
    #[to_metadata]
    pub path: String,

    /// Whether selection mode is enabled
    #[to_metadata]
    pub enabled: bool,

    /// Parsed selection data (if any)
    #[to_result(skip_if_none)]
    pub selection: Option<SelectionData>,

    /// Message template for formatting responses
    #[to_message(message_template = "Grab selection fetched from {path}")]
    pub message_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SelectionData {
    pub entity: EntitySummary,
    #[serde(default)]
    pub hierarchy: Vec<String>,
    pub cursor: Option<CursorSummary>,
    pub target: SelectionTargetSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EntitySummary {
    pub id: u32,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CursorSummary {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SelectionTargetSummary {
    Ui {
        rect: RectSummary,
        text: Option<String>,
    },
    World {
        position: [f32; 3],
        bounds: Option<BoundsSummary>,
        mesh: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RectSummary {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoundsSummary {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SelectionSummaryFile {
    pub enabled: bool,
    pub selection: Option<SelectionData>,
}

#[derive(ToolFn)]
#[tool_fn(params = "GrabSelectionParams", output = "GrabSelectionResult")]
pub struct GrabSelection;

#[allow(clippy::unused_async)]
async fn handle_impl(params: GrabSelectionParams) -> crate::error::Result<GrabSelectionResult> {
    let path = resolve_path(params.path.as_deref());

    if !path.exists() {
        if params.fail_if_absent {
            return Err(Error::missing(&format!(
                "grab selection file at {}",
                path.display()
            ))
            .into());
        }

        return Ok(GrabSelectionResult::new(path.display().to_string(), false, None));
    }

    let contents = fs::read_to_string(&path)
        .map_err(|e| Error::io_failed("read grab selection file", &path, &e))?;

    let summary: SelectionSummaryFile = serde_json::from_str(&contents)
        .map_err(|e| Error::failed_to("parse grab selection", e))?;

    if params.require_enabled && !summary.enabled {
        return Err(Error::invalid("enabled", "selection capture is disabled").into());
    }

    Ok(GrabSelectionResult::new(
        path.display().to_string(),
        summary.enabled,
        summary.selection,
    ))
}

fn resolve_path(arg: Option<&str>) -> PathBuf {
    if let Some(p) = arg {
        return PathBuf::from(p);
    }

    if let Ok(env_path) = env::var(ENV_SELECTION_PATH) {
        if !env_path.is_empty() {
            return PathBuf::from(env_path);
        }
    }

    PathBuf::from(DEFAULT_SELECTION_PATH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use tempfile::TempDir;

    fn write_file(dir: &TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn resolves_default_path_when_none() {
        let path = resolve_path(None);
        assert!(path.ends_with(DEFAULT_SELECTION_PATH));
    }

    #[test]
    fn parses_ui_selection() {
        let dir = TempDir::new().unwrap();
        let json = r#"{
  "enabled": true,
  "selection": {
    "entity": {"id": 1, "name": "Button"},
    "hierarchy": ["Root", "Button"],
    "cursor": {"x": 10.0, "y": 20.0},
    "target": {
      "type": "ui",
      "rect": {"min": [0.0, 0.0], "max": [100.0, 50.0], "z": 1.0},
      "text": "Click"
    }
  }
}"#;
        let path = write_file(&dir, "target/ai-selection/selection.json", json);

        let result = block_on(handle_impl(GrabSelectionParams {
            path: Some(path.to_string_lossy().to_string()),
            require_enabled: false,
            fail_if_absent: true,
        }))
        .unwrap();

        assert!(result.enabled);
        let sel = result.selection.unwrap();
        assert_eq!(sel.entity.id, 1);
        assert_eq!(sel.hierarchy, vec!["Root", "Button"]);
        match sel.target {
            SelectionTargetSummary::Ui { rect, text } => {
                assert_eq!(rect.min, [0.0, 0.0]);
                assert_eq!(rect.max, [100.0, 50.0]);
                assert_eq!(rect.z, 1.0);
                assert_eq!(text.as_deref(), Some("Click"));
            },
            _ => panic!("expected ui target"),
        }
    }

    #[test]
    fn world_selection_bounds_optional() {
        let dir = TempDir::new().unwrap();
        let json = r#"{
  "enabled": false,
  "selection": {
    "entity": {"id": 5, "name": null},
    "hierarchy": [],
    "cursor": null,
    "target": {
      "type": "world",
      "position": [1.0, 2.0, 3.0],
      "bounds": null,
      "mesh": "mesh-123"
    }
  }
}"#;
        let path = write_file(&dir, "sel.json", json);

        let result = block_on(handle_impl(GrabSelectionParams {
            path: Some(path.to_string_lossy().to_string()),
            require_enabled: false,
            fail_if_absent: true,
        }))
        .unwrap();

        assert!(!result.enabled);
        let sel = result.selection.unwrap();
        match sel.target {
            SelectionTargetSummary::World { position, bounds, mesh } => {
                assert_eq!(position, [1.0, 2.0, 3.0]);
                assert!(bounds.is_none());
                assert_eq!(mesh.as_deref(), Some("mesh-123"));
            },
            _ => panic!("expected world target"),
        }
    }

    #[test]
    fn missing_file_allowed_when_flag_false() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope.json");

        let result = block_on(handle_impl(GrabSelectionParams {
            path: Some(missing.to_string_lossy().to_string()),
            require_enabled: false,
            fail_if_absent: false,
        }))
        .unwrap();

        assert!(!result.enabled);
        assert!(result.selection.is_none());
    }
}
