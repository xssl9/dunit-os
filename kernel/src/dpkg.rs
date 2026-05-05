use alloc::vec::Vec;
use alloc::string::String;

pub struct Package {
    name: &'static str,
    version: &'static str,
    description: &'static str,
}

static mut INSTALLED_PACKAGES: Option<Vec<Package>> = None;
static mut INITIALIZED: bool = false;

pub fn init() {
    unsafe {
        INSTALLED_PACKAGES = Some(Vec::new());
        INITIALIZED = true;
    }
}

fn ensure_base_packages() {
    unsafe {
        if !INITIALIZED {
            return;
        }
        if let Some(ref mut packages) = INSTALLED_PACKAGES {
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
        if let Some(ref packages) = INSTALLED_PACKAGES {
            packages.iter().any(|p| p.name == name)
        } else {
            false
        }
    }
}

pub fn install(name: &'static str, version: &'static str, description: &'static str) -> bool {
    unsafe {
        if let Some(ref mut packages) = INSTALLED_PACKAGES {
            if !is_installed(name) {
                packages.push(Package { name, version, description });
                true
            } else {
                false
            }
        } else {
            false
        }
    }
}

pub fn remove(name: &str) -> bool {
    unsafe {
        if let Some(ref mut packages) = INSTALLED_PACKAGES {
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
        if let Some(ref packages) = INSTALLED_PACKAGES {
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
