use std::collections::HashMap;
use std::sync::Arc;
use retoc::iostore::open;
use retoc::{Config, FIoContainerId, FPackageId};

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let name = std::env::args().nth(2).unwrap();
    let cfg = Config { aes_keys: HashMap::new(), container_header_version_override: None, toc_version_override: None };
    let store = open(path, Arc::new(cfg)).unwrap();
    let id = FPackageId(FIoContainerId::from_name(&name).0);
    match store.package_store_entry(id) {
        Some(e) => {
            println!("export_count={} bundle_count={} imported_packages={}", e.export_count, e.export_bundle_count, e.imported_packages.len());
            for p in &e.imported_packages { println!("  imp {}", p); }
        }
        None => println!("no entry"),
    }
}
