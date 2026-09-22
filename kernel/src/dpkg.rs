use alloc::string::String;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

pub struct Package {
    name: &'static str,
    version: &'static str,
    description: &'static str,
}

/// Список установленных пакетов. Раньше `static mut Option<Vec<..>>`; теперь
/// `UnsafeCell`-newtype без `static mut`. Менеджер пакетов работает кооперативно
/// на одном CPU. Флаг инициализации вынесен в атомик.
struct PackagesCell(UnsafeCell<Option<Vec<Package>>>);
unsafe impl Sync for PackagesCell {}
static INSTALLED_PACKAGES: PackagesCell = PackagesCell(UnsafeCell::new(None));
static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub fn init() {
    unsafe {
        *INSTALLED_PACKAGES.0.get() = Some(Vec::new());
    }
    INITIALIZED.store(true, Ordering::Relaxed);
}

fn ensure_base_packages() {
    unsafe {
        if !INITIALIZED.load(Ordering::Relaxed) {
            return;
        }
        if let Some(packages) = (*INSTALLED_PACKAGES.0.get()).as_mut() {
            if packages.is_empty() {
                packages.push(Package {
                    name: "dunit-base",
                    version: "1.0.0",
                    description: "Base system",
                });
                packages.push(Package {
                    name: "dunit-kernel",
                    version: "1.0.0",
                    description: "Kernel",
                });
                packages.push(Package {
                    name: "dunit-utils",
                    version: "1.0.0",
                    description: "System utilities",
                });
                packages.push(Package {
                    name: "dunit-drivers",
                    version: "1.0.0",
                    description: "Hardware drivers",
                });
            }
        }
    }
}

pub fn is_installed(name: &str) -> bool {
    unsafe {
        if let Some(packages) = (*INSTALLED_PACKAGES.0.get()).as_ref() {
            packages.iter().any(|p| p.name == name)
        } else {
            false
        }
    }
}

pub fn install(name: &'static str, version: &'static str, description: &'static str) -> bool {
    if is_installed(name) {
        return false;
    }
    unsafe {
        if let Some(packages) = (*INSTALLED_PACKAGES.0.get()).as_mut() {
            packages.push(Package {
                name,
                version,
                description,
            });
            true
        } else {
            false
        }
    }
}

pub fn remove(name: &str) -> bool {
    unsafe {
        if let Some(packages) = (*INSTALLED_PACKAGES.0.get()).as_mut() {
            let len_before = packages.len();
            packages.retain(|p| p.name != name);
            packages.len() < len_before
        } else {
            false
        }
    }
}

pub fn list() -> String {
    ensure_base_packages();
    unsafe {
        if let Some(packages) = (*INSTALLED_PACKAGES.0.get()).as_ref() {
            let mut result = String::from("Installed packages:\n");
            for pkg in packages {
                result.push_str("  ");
                result.push_str(pkg.name);
                result.push_str("  ");
                result.push_str(pkg.version);
                result.push_str("  ");
                result.push_str(pkg.description);
                result.push_str("\n");
            }
            result
        } else {
            String::from("Package manager not initialized")
        }
    }
}

pub fn get_available_package(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match name {
        "vim" => Some(("vim", "9.0.0", "Text editor")),
        "gcc" => Some(("gcc", "13.2.0", "C compiler")),
        "python" => Some(("python", "3.12.0", "Python interpreter")),
        "rust" => Some(("rust", "1.75.0", "Rust compiler")),
        "git" => Some(("git", "2.43.0", "Version control")),
        "htop" => Some(("htop", "3.3.0", "Process monitor")),
        _ => None,
    }
}
