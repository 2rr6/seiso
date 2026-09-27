use seiso_config::{CliOverrides, Config};
use seiso_rules::{CheckContext, PathStatus, WorkspaceFiles, check, check_with_files};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

struct Inventory {
    paths: BTreeSet<PathBuf>,
}
impl WorkspaceFiles for Inventory {
    fn status(&self, _root: &Path, target: &Path) -> PathStatus {
        if self.paths.contains(target) {
            PathStatus::Exists
        } else {
            PathStatus::Missing
        }
    }
}

#[test]
fn frozen_inventory_uses_the_same_path_resolution_as_the_filesystem() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("docs")).unwrap();
    std::fs::write(root.path().join("配置.md"), "# Configuration").unwrap();
    let document = seiso_md::parse("[existing](/%E9%85%8D%E7%BD%AE.md#anchor)\n\n[missing](missing.md)\n\n[outside](../../outside.md)\n").unwrap();
    let config = Config::parse("preview=true\n[lint]\nselect=['LNK001']", root.path()).unwrap();
    let context = CheckContext {
        document: &document,
        filename: "docs/test.md",
        path: &root.path().join("docs/test.md"),
        workspace_root: root.path(),
        config: &config,
        overrides: &CliOverrides::default(),
    };
    let inventory = Inventory {
        paths: [root.path().join("配置.md")].into(),
    };
    assert_eq!(
        check(&context).unwrap().diagnostics,
        check_with_files(&context, &inventory).unwrap().diagnostics
    );
    assert_eq!(check(&context).unwrap().diagnostics.len(), 1);
}
