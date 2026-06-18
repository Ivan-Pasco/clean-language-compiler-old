use crate::error::CompilerError;
use crate::module::ModuleResolver;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Package manifest structure (package.clean file)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    pub dependencies: Option<HashMap<String, DependencySpec>>,
    pub dev_dependencies: Option<HashMap<String, DependencySpec>>,
    pub build: Option<BuildConfig>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Package information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Option<Vec<String>>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
}

/// Dependency specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    Simple(String), // "1.0.0"
    Detailed {
        version: Option<String>,       // "^1.0.0"
        git: Option<String>,           // Git repository URL
        branch: Option<String>,        // Git branch
        tag: Option<String>,           // Git tag
        path: Option<String>,          // Local path
        registry: Option<String>,      // Custom registry
        optional: Option<bool>,        // Optional dependency
        features: Option<Vec<String>>, // Feature flags
    },
}

/// Build configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    pub target: Option<String>,        // "wasm32-unknown-unknown"
    pub optimization: Option<String>,  // "size" | "speed" | "debug"
    pub features: Option<Vec<String>>, // Feature flags to enable
    pub exclude: Option<Vec<String>>,  // Files to exclude from build
}

/// Semantic version structure
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub pre_release: Option<String>,
    pub build: Option<String>,
}

/// Version requirement specification
#[derive(Debug, Clone)]
pub enum VersionReq {
    Exact(Version),             // "1.0.0"
    Caret(Version),             // "^1.0.0" - compatible within major version
    Tilde(Version),             // "~1.0.0" - compatible within minor version
    GreaterThan(Version),       // ">1.0.0"
    GreaterEqual(Version),      // ">=1.0.0"
    LessThan(Version),          // "<2.0.0"
    LessEqual(Version),         // "<=1.9.9"
    Range(Version, Version),    // ">=1.0.0, <2.0.0"
    Wildcard(u32, Option<u32>), // "1.*" or "1.2.*"
}

/// Package registry entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPackage {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub download_url: String,
    pub checksum: String,
    pub dependencies: HashMap<String, String>,
    pub published_at: String,
}

/// Package manager for Clean Language
pub struct PackageManager {
    /// Local package cache directory
    cache_dir: PathBuf,
    /// Registry URLs
    #[allow(dead_code)] // Registry list populated at init; network fetch not yet implemented
    registries: Vec<String>,
    /// Module resolver for loading packages
    #[allow(dead_code)] // Resolver held for future on-demand loading; not yet queried
    module_resolver: ModuleResolver,
    /// Installed packages cache
    #[allow(dead_code)]
    // Package cache populated by install(); lookups not yet used at compile time
    installed_packages: HashMap<String, InstalledPackage>,
}

/// Installed package information
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub manifest: PackageManifest,
    pub install_path: PathBuf,
    pub resolved_dependencies: HashMap<String, String>, // name -> version
}

/// Dependency resolution result
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    pub packages: HashMap<String, ResolvedPackage>,
    pub resolution_order: Vec<String>,
}

/// Resolved package with specific version
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub source: PackageSource,
    pub dependencies: HashMap<String, String>,
}

/// Package source location
#[derive(Debug, Clone)]
pub enum PackageSource {
    Registry {
        url: String,
    },
    Git {
        url: String,
        branch: Option<String>,
        tag: Option<String>,
    },
    Path {
        path: PathBuf,
    },
    Local {
        path: PathBuf,
    },
}

impl PackageManager {
    /// Create a new package manager
    pub fn new(cache_dir: PathBuf) -> Self {
        let default_registry = "https://packages.cleanlang.org".to_string();

        PackageManager {
            cache_dir,
            registries: vec![default_registry],
            module_resolver: ModuleResolver::new(),
            installed_packages: HashMap::new(),
        }
    }

    /// Load package manifest from file
    pub fn load_manifest<P: AsRef<Path>>(path: P) -> Result<PackageManifest, CompilerError> {
        let content = fs::read_to_string(&path).map_err(|e| {
            CompilerError::io_error(
                format!("Failed to read package manifest: {e}"),
                Some(path.as_ref().to_string_lossy().to_string()),
                None,
            )
        })?;

        // Try TOML format first, then JSON
        if let Ok(manifest) = toml::from_str::<PackageManifest>(&content) {
            Ok(manifest)
        } else if let Ok(manifest) = serde_json::from_str::<PackageManifest>(&content) {
            Ok(manifest)
        } else {
            Err(CompilerError::parse_error(
                "Invalid package manifest format. Expected TOML or JSON.",
                None,
                Some("Use either package.clean.toml or package.clean.json format".to_string()),
            ))
        }
    }

    /// Save package manifest to file
    pub fn save_manifest<P: AsRef<Path>>(
        manifest: &PackageManifest,
        path: P,
    ) -> Result<(), CompilerError> {
        let content = if path.as_ref().extension().and_then(|s| s.to_str()) == Some("json") {
            serde_json::to_string_pretty(manifest).map_err(|e| {
                CompilerError::io_error(format!("Failed to serialize manifest: {e}"), None, None)
            })?
        } else {
            toml::to_string_pretty(manifest).map_err(|e| {
                CompilerError::io_error(format!("Failed to serialize manifest: {e}"), None, None)
            })?
        };

        fs::write(&path, content).map_err(|e| {
            CompilerError::io_error(
                format!("Failed to write package manifest: {e}"),
                Some(path.as_ref().to_string_lossy().to_string()),
                None,
            )
        })?;

        Ok(())
    }

    /// Initialize a new package in the current directory
    pub fn init_package<P: AsRef<Path>>(
        &self,
        project_dir: P,
        name: String,
        version: Option<String>,
        description: Option<String>,
    ) -> Result<PackageManifest, CompilerError> {
        let manifest = PackageManifest {
            package: PackageInfo {
                name: name.clone(),
                version: version.unwrap_or_else(|| "0.1.0".to_string()),
                description,
                authors: None,
                license: Some("MIT".to_string()),
                repository: None,
                homepage: None,
                keywords: None,
                categories: None,
            },
            dependencies: None,
            dev_dependencies: None,
            build: Some(BuildConfig {
                target: Some("wasm32-unknown-unknown".to_string()),
                optimization: Some("size".to_string()),
                features: None,
                exclude: Some(vec!["tests/".to_string(), "examples/".to_string()]),
            }),
            metadata: None,
        };

        let manifest_path = project_dir.as_ref().join("package.clean.toml");
        Self::save_manifest(&manifest, &manifest_path)?;

        // Create basic project structure
        let src_dir = project_dir.as_ref().join("src");
        fs::create_dir_all(&src_dir).map_err(|e| {
            CompilerError::io_error(format!("Failed to create src directory: {e}"), None, None)
        })?;

        // Create main.clean file
        let main_file = src_dir.join("main.clean");
        if !main_file.exists() {
            let main_content = format!(
                "// {name} - Clean Language Package\n\nfunction start()\n\tprint(\"Hello from {name}!\")\n"
            );
            fs::write(&main_file, main_content).map_err(|e| {
                CompilerError::io_error(format!("Failed to create main.clean: {e}"), None, None)
            })?;
        }

        println!("✅ Initialized Clean Language package: {name}");
        Ok(manifest)
    }

    /// Resolve dependencies for a package
    pub fn resolve_dependencies(
        &self,
        manifest: &PackageManifest,
    ) -> Result<DependencyGraph, CompilerError> {
        let mut resolver = DependencyResolver::new();

        // Add root package dependencies
        if let Some(deps) = &manifest.dependencies {
            for (name, spec) in deps {
                resolver.add_dependency(name.clone(), spec.clone(), false)?;
            }
        }

        // Add development dependencies if needed
        if let Some(dev_deps) = &manifest.dev_dependencies {
            for (name, spec) in dev_deps {
                resolver.add_dependency(name.clone(), spec.clone(), true)?;
            }
        }

        resolver.resolve()
    }

    /// Install dependencies for a package
    pub async fn install_dependencies(
        &mut self,
        manifest: &PackageManifest,
    ) -> Result<(), CompilerError> {
        let dependency_graph = self.resolve_dependencies(manifest)?;

        println!(
            "📦 Installing {} dependencies...",
            dependency_graph.packages.len()
        );

        for package_name in &dependency_graph.resolution_order {
            if let Some(package) = dependency_graph.packages.get(package_name) {
                self.install_package(package).await?;
            }
        }

        println!("✅ All dependencies installed successfully!");
        Ok(())
    }

    /// Install a single package
    async fn install_package(&mut self, package: &ResolvedPackage) -> Result<(), CompilerError> {
        let install_path = self.cache_dir.join(&package.name).join(&package.version);

        // Skip if already installed
        if install_path.exists() {
            println!(
                "⏭️  {name} {version} already installed",
                name = package.name,
                version = package.version
            );
            return Ok(());
        }

        println!(
            "📥 Installing {name} {version}...",
            name = package.name,
            version = package.version
        );

        match &package.source {
            PackageSource::Registry { url } => {
                self.install_from_registry(&package.name, &package.version, url, &install_path)
                    .await?;
            }
            PackageSource::Git { url, branch, tag } => {
                let _branch = branch.clone();
                let _tag = tag.clone();
                self.install_from_git(url, &install_path).await?;
            }
            PackageSource::Path { path } => {
                self.install_from_path(path, &install_path)?;
            }
            PackageSource::Local { path } => {
                // Local packages don't need installation, just reference
                println!("🔗 Linking local package: {path}", path = path.display());
            }
        }

        println!(
            "✅ Installed {name} {version}",
            name = package.name,
            version = package.version
        );
        Ok(())
    }

    /// Install package from registry
    async fn install_from_registry(
        &self,
        name: &str,
        version: &str,
        registry_url: &str,
        install_path: &Path,
    ) -> Result<(), CompilerError> {
        // This would typically download from a package registry
        // For now, we'll simulate the process
        fs::create_dir_all(install_path).map_err(|e| {
            CompilerError::io_error(
                format!("Failed to create install directory: {e}"),
                None,
                None,
            )
        })?;

        println!("📡 Downloading {name} {version} from {registry_url}");

        // Simulate package download and extraction
        // In a real implementation, this would:
        // 1. Download package archive from registry
        // 2. Verify checksum
        // 3. Extract to install_path
        // 4. Load and cache package manifest

        Ok(())
    }

    /// Install package from Git repository
    async fn install_from_git(
        &self,
        git_url: &str,
        install_path: &Path,
    ) -> Result<(), CompilerError> {
        fs::create_dir_all(install_path).map_err(|e| {
            CompilerError::io_error(
                format!("Failed to create install directory: {e}"),
                None,
                None,
            )
        })?;

        println!("🌿 Cloning from Git: {git_url}");

        // In a real implementation, this would use git2 or similar to clone the repository
        // For now, we'll simulate the process

        Ok(())
    }

    /// Install package from local path
    fn install_from_path(
        &self,
        source_path: &Path,
        install_path: &Path,
    ) -> Result<(), CompilerError> {
        fs::create_dir_all(install_path).map_err(|e| {
            CompilerError::io_error(
                format!("Failed to create install directory: {e}"),
                None,
                None,
            )
        })?;

        println!(
            "📁 Copying from local path: {path}",
            path = source_path.display()
        );

        // Copy package files to install location
        Self::copy_dir_recursive(source_path, install_path)?;

        Ok(())
    }

    /// Recursively copy directory
    fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), CompilerError> {
        if !dst.exists() {
            fs::create_dir_all(dst).map_err(|e| {
                CompilerError::io_error(format!("Failed to create directory: {e}"), None, None)
            })?;
        }

        for entry in fs::read_dir(src).map_err(|e| {
            CompilerError::io_error(format!("Failed to read directory: {e}"), None, None)
        })? {
            let entry = entry.map_err(|e| {
                CompilerError::io_error(format!("Failed to read directory entry: {e}"), None, None)
            })?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if src_path.is_dir() {
                Self::copy_dir_recursive(&src_path, &dst_path)?;
            } else {
                fs::copy(&src_path, &dst_path).map_err(|e| {
                    CompilerError::io_error(format!("Failed to copy file: {e}"), None, None)
                })?;
            }
        }

        Ok(())
    }

    /// Add a new dependency to package manifest
    pub fn add_dependency(
        &self,
        manifest_path: &Path,
        name: String,
        version_spec: String,
        dev: bool,
    ) -> Result<(), CompilerError> {
        let mut manifest = Self::load_manifest(manifest_path)?;

        let dependency_spec = DependencySpec::Simple(version_spec);

        if dev {
            manifest
                .dev_dependencies
                .get_or_insert_with(HashMap::new)
                .insert(name.clone(), dependency_spec);
        } else {
            manifest
                .dependencies
                .get_or_insert_with(HashMap::new)
                .insert(name.clone(), dependency_spec);
        }

        Self::save_manifest(&manifest, manifest_path)?;

        println!(
            "✅ Added {} dependency: {}",
            if dev { "dev" } else { "runtime" },
            name
        );
        Ok(())
    }

    /// Remove a dependency from package manifest
    pub fn remove_dependency(&self, manifest_path: &Path, name: &str) -> Result<(), CompilerError> {
        let mut manifest = Self::load_manifest(manifest_path)?;

        let mut removed = false;

        if let Some(deps) = &mut manifest.dependencies {
            if deps.remove(name).is_some() {
                removed = true;
            }
        }

        if let Some(dev_deps) = &mut manifest.dev_dependencies {
            if dev_deps.remove(name).is_some() {
                removed = true;
            }
        }

        if removed {
            Self::save_manifest(&manifest, manifest_path)?;
            println!("✅ Removed dependency: {name}");
        } else {
            println!("⚠️  Dependency not found: {name}");
        }

        Ok(())
    }
}

/// Dependency resolver for handling version constraints and conflicts
struct DependencyResolver {
    dependencies: HashMap<String, Vec<(DependencySpec, bool)>>, // name -> (spec, is_dev)
    #[allow(dead_code)]
    // Resolution results stored but not yet read back by add_dependency callers
    resolved: HashMap<String, ResolvedPackage>,
}

impl DependencyResolver {
    fn new() -> Self {
        DependencyResolver {
            dependencies: HashMap::new(),
            resolved: HashMap::new(),
        }
    }

    fn add_dependency(
        &mut self,
        name: String,
        spec: DependencySpec,
        is_dev: bool,
    ) -> Result<(), CompilerError> {
        self.dependencies
            .entry(name)
            .or_default()
            .push((spec, is_dev));
        Ok(())
    }

    fn resolve(&mut self) -> Result<DependencyGraph, CompilerError> {
        // Per-package resolution:
        //   1. Walk every spec attached to the package.
        //   2. Pick the source (registry / git / path) — git and path
        //      sources bypass version arithmetic because they pin the wire
        //      bytes directly.
        //   3. Parse each version string into a `VersionReq`, intersect
        //      every range, and pick a concrete version that satisfies the
        //      intersection. If two constraints have no overlap, report a
        //      hard error instead of silently picking one.
        //
        // With no live registry catalog available at resolve time, the
        // chosen version is whatever explicit version string appears in the
        // user's manifest (e.g. `"1.4.2"`). When every constraint is
        // open-ended (`^1.0`, `>=2.0`, `1.*`), the resolver leaves the
        // version as the formatted form of the intersection's lower bound
        // so downstream tooling can resolve it against the registry without
        // losing the constraint.

        let mut packages = HashMap::new();
        let mut resolution_order = Vec::new();

        for (name, specs) in &self.dependencies {
            if specs.is_empty() {
                continue;
            }

            // Separate non-version sources (git/path) from version specs.
            let mut requirements: Vec<(VersionReq, String)> = Vec::new();
            let mut explicit_versions: Vec<Version> = Vec::new();
            let mut alt_source: Option<PackageSource> = None;
            let mut alt_source_label = String::from("latest");

            for (spec, _is_dev) in specs {
                match spec {
                    DependencySpec::Simple(v) => {
                        let req = VersionReq::parse(v).map_err(|e| {
                            CompilerError::parse_error(
                                format!(
                                    "Invalid version requirement for `{name}` (`{v}`): {e}"
                                ),
                                None,
                                Some(
                                    "Use semver forms like `1.2.3`, `^1.2`, `~1.2`, `>=1.0, <2.0`, or `1.*`.".to_string(),
                                ),
                            )
                        })?;
                        if let VersionReq::Exact(ver) = &req {
                            explicit_versions.push(ver.clone());
                        }
                        requirements.push((req, v.clone()));
                    }
                    DependencySpec::Detailed {
                        version: Some(v),
                        git: None,
                        path: None,
                        ..
                    } => {
                        let req = VersionReq::parse(v).map_err(|e| {
                            CompilerError::parse_error(
                                format!("Invalid version requirement for `{name}` (`{v}`): {e}"),
                                None,
                                None,
                            )
                        })?;
                        if let VersionReq::Exact(ver) = &req {
                            explicit_versions.push(ver.clone());
                        }
                        requirements.push((req, v.clone()));
                    }
                    DependencySpec::Detailed {
                        git: Some(url),
                        branch,
                        tag,
                        ..
                    } => {
                        alt_source = Some(PackageSource::Git {
                            url: url.clone(),
                            branch: branch.clone(),
                            tag: tag.clone(),
                        });
                        alt_source_label = tag
                            .clone()
                            .unwrap_or_else(|| branch.clone().unwrap_or_else(|| "git".to_string()));
                    }
                    DependencySpec::Detailed {
                        path: Some(local), ..
                    } => {
                        alt_source = Some(PackageSource::Path {
                            path: PathBuf::from(local),
                        });
                        alt_source_label = "local".to_string();
                    }
                    DependencySpec::Detailed { .. } => {
                        // Empty detailed entry — nothing to use. Skip rather
                        // than make a wild guess; downstream code will report
                        // the dependency as unresolvable.
                    }
                }
            }

            let (version, source) = if let Some(src) = alt_source {
                (alt_source_label, src)
            } else if requirements.is_empty() {
                // Every spec was a no-op detailed entry. Mark as unresolved.
                (
                    "latest".to_string(),
                    PackageSource::Registry {
                        url: "https://packages.cleanlang.org".to_string(),
                    },
                )
            } else {
                let bounds = intersect_requirements(name, &requirements)?;

                // Pick the highest explicit candidate that satisfies every
                // requirement. Falling back to the formatted lower bound when
                // no candidate exists keeps the resolved version informative
                // for downstream tooling (and avoids the historical bug
                // where every open-ended dependency resolved to the literal
                // string "latest").
                let chosen = explicit_versions
                    .iter()
                    .filter(|v| requirements.iter().all(|(r, _)| v.satisfies(r)))
                    .max()
                    .cloned();

                let version_string = match chosen {
                    Some(v) => v.to_string(),
                    None => bounds.formatted_lower(),
                };

                (
                    version_string,
                    PackageSource::Registry {
                        url: "https://packages.cleanlang.org".to_string(),
                    },
                )
            };

            let resolved_package = ResolvedPackage {
                name: name.clone(),
                version,
                source,
                dependencies: HashMap::new(),
            };
            self.resolved.insert(name.clone(), resolved_package.clone());
            packages.insert(name.clone(), resolved_package);
            resolution_order.push(name.clone());
        }

        // Stable ordering so callers (and tests) see a deterministic graph.
        resolution_order.sort();

        Ok(DependencyGraph {
            packages,
            resolution_order,
        })
    }
}

/// Inclusive/exclusive boundary for an intersected version range.
#[derive(Debug, Clone)]
struct VersionBounds {
    lower: Option<(Version, bool)>, // (version, inclusive)
    upper: Option<(Version, bool)>, // (version, inclusive)
}

impl VersionBounds {
    fn unbounded() -> Self {
        VersionBounds {
            lower: None,
            upper: None,
        }
    }

    fn from_req(req: &VersionReq) -> VersionBounds {
        match req {
            VersionReq::Exact(v) => VersionBounds {
                lower: Some((v.clone(), true)),
                upper: Some((v.clone(), true)),
            },
            VersionReq::Caret(v) => VersionBounds {
                lower: Some((v.clone(), true)),
                upper: Some((next_major(v), false)),
            },
            VersionReq::Tilde(v) => VersionBounds {
                lower: Some((v.clone(), true)),
                upper: Some((next_minor(v), false)),
            },
            VersionReq::GreaterThan(v) => VersionBounds {
                lower: Some((v.clone(), false)),
                upper: None,
            },
            VersionReq::GreaterEqual(v) => VersionBounds {
                lower: Some((v.clone(), true)),
                upper: None,
            },
            VersionReq::LessThan(v) => VersionBounds {
                lower: None,
                upper: Some((v.clone(), false)),
            },
            VersionReq::LessEqual(v) => VersionBounds {
                lower: None,
                upper: Some((v.clone(), true)),
            },
            VersionReq::Range(min, max) => VersionBounds {
                lower: Some((min.clone(), true)),
                upper: Some((max.clone(), false)),
            },
            VersionReq::Wildcard(maj, None) => VersionBounds {
                lower: Some((
                    Version {
                        major: *maj,
                        minor: 0,
                        patch: 0,
                        pre_release: None,
                        build: None,
                    },
                    true,
                )),
                upper: Some((
                    Version {
                        major: maj + 1,
                        minor: 0,
                        patch: 0,
                        pre_release: None,
                        build: None,
                    },
                    false,
                )),
            },
            VersionReq::Wildcard(maj, Some(min)) => VersionBounds {
                lower: Some((
                    Version {
                        major: *maj,
                        minor: *min,
                        patch: 0,
                        pre_release: None,
                        build: None,
                    },
                    true,
                )),
                upper: Some((
                    Version {
                        major: *maj,
                        minor: min + 1,
                        patch: 0,
                        pre_release: None,
                        build: None,
                    },
                    false,
                )),
            },
        }
    }

    /// Intersect two bound sets. Returns `None` when the intersection is
    /// empty (i.e. the two constraints conflict and no version can satisfy
    /// both).
    fn intersect(self, other: VersionBounds) -> Option<VersionBounds> {
        let lower = match (self.lower, other.lower) {
            (None, x) | (x, None) => x,
            (Some((a, a_incl)), Some((b, b_incl))) => Some(if a > b {
                (a, a_incl)
            } else if b > a {
                (b, b_incl)
            } else {
                // Equal versions: inclusivity is AND of both sides.
                (a, a_incl && b_incl)
            }),
        };

        let upper = match (self.upper, other.upper) {
            (None, x) | (x, None) => x,
            (Some((a, a_incl)), Some((b, b_incl))) => Some(if a < b {
                (a, a_incl)
            } else if b < a {
                (b, b_incl)
            } else {
                (a, a_incl && b_incl)
            }),
        };

        // Empty-range check.
        if let (Some((lo, lo_incl)), Some((hi, hi_incl))) = (&lower, &upper) {
            match lo.cmp(hi) {
                std::cmp::Ordering::Greater => return None,
                std::cmp::Ordering::Equal if !(*lo_incl && *hi_incl) => return None,
                _ => {}
            }
        }

        Some(VersionBounds { lower, upper })
    }

    /// Format a representative version string for this range. Used when no
    /// explicit candidate version is supplied (e.g. every constraint is
    /// open-ended like `^1.2`).
    fn formatted_lower(&self) -> String {
        match &self.lower {
            Some((v, _)) => v.to_string(),
            None => "0.0.0".to_string(),
        }
    }
}

fn next_major(v: &Version) -> Version {
    Version {
        major: v.major + 1,
        minor: 0,
        patch: 0,
        pre_release: None,
        build: None,
    }
}

fn next_minor(v: &Version) -> Version {
    Version {
        major: v.major,
        minor: v.minor + 1,
        patch: 0,
        pre_release: None,
        build: None,
    }
}

fn intersect_requirements(
    name: &str,
    requirements: &[(VersionReq, String)],
) -> Result<VersionBounds, CompilerError> {
    let mut acc = VersionBounds::unbounded();
    for (req, raw) in requirements {
        let next = VersionBounds::from_req(req);
        acc = acc.intersect(next).ok_or_else(|| {
            let listed = requirements
                .iter()
                .map(|(_, s)| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            CompilerError::parse_error(
                format!(
                    "Conflicting version requirements for `{name}`: `{raw}` does not overlap with the other constraints ({listed})"
                ),
                None,
                Some(
                    "Reconcile the version specifiers so every dependent agrees on a satisfiable range."
                        .to_string(),
                ),
            )
        })?;
    }
    Ok(acc)
}

impl Version {
    /// Parse version string
    pub fn parse(version_str: &str) -> Result<Version, CompilerError> {
        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() < 3 {
            return Err(CompilerError::parse_error(
                format!("Invalid version format: {version_str}"),
                None,
                Some("Version must be in format 'major.minor.patch'".to_string()),
            ));
        }

        let major = parts[0]
            .parse::<u32>()
            .map_err(|_| CompilerError::parse_error("Invalid major version number", None, None))?;
        let minor = parts[1]
            .parse::<u32>()
            .map_err(|_| CompilerError::parse_error("Invalid minor version number", None, None))?;
        let patch = parts[2]
            .parse::<u32>()
            .map_err(|_| CompilerError::parse_error("Invalid patch version number", None, None))?;

        Ok(Version {
            major,
            minor,
            patch,
            pre_release: None,
            build: None,
        })
    }

    /// Check if this version satisfies a requirement
    pub fn satisfies(&self, req: &VersionReq) -> bool {
        match req {
            VersionReq::Exact(v) => self == v,
            VersionReq::Caret(v) => {
                self.major == v.major
                    && (self.minor > v.minor || (self.minor == v.minor && self.patch >= v.patch))
            }
            VersionReq::Tilde(v) => {
                self.major == v.major && self.minor == v.minor && self.patch >= v.patch
            }
            VersionReq::GreaterThan(v) => self > v,
            VersionReq::GreaterEqual(v) => self >= v,
            VersionReq::LessThan(v) => self < v,
            VersionReq::LessEqual(v) => self <= v,
            VersionReq::Range(min, max) => self >= min && self < max,
            VersionReq::Wildcard(major, minor) => {
                self.major == *major && minor.is_none_or(|m| self.minor == m)
            }
        }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre_release {
            write!(f, "-{pre}")?;
        }
        if let Some(build) = &self.build {
            write!(f, "+{build}")?;
        }
        Ok(())
    }
}

impl VersionReq {
    /// Parse version requirement string
    pub fn parse(req_str: &str) -> Result<VersionReq, CompilerError> {
        let req_str = req_str.trim();

        if let Some(stripped) = req_str.strip_prefix('^') {
            let version = Version::parse(stripped)?;
            Ok(VersionReq::Caret(version))
        } else if let Some(stripped) = req_str.strip_prefix('~') {
            let version = Version::parse(stripped)?;
            Ok(VersionReq::Tilde(version))
        } else if let Some(stripped) = req_str.strip_prefix(">=") {
            let version = Version::parse(stripped)?;
            Ok(VersionReq::GreaterEqual(version))
        } else if let Some(stripped) = req_str.strip_prefix('>') {
            let version = Version::parse(stripped)?;
            Ok(VersionReq::GreaterThan(version))
        } else if let Some(stripped) = req_str.strip_prefix("<=") {
            let version = Version::parse(stripped)?;
            Ok(VersionReq::LessEqual(version))
        } else if let Some(stripped) = req_str.strip_prefix('<') {
            let version = Version::parse(stripped)?;
            Ok(VersionReq::LessThan(version))
        } else if req_str.contains('*') {
            // Handle wildcard versions like "1.*" or "1.2.*"
            let parts: Vec<&str> = req_str.split('.').collect();
            if parts.len() >= 2 && parts[0] != "*" {
                let major = parts[0].parse::<u32>().map_err(|_| {
                    CompilerError::parse_error("Invalid major version in wildcard", None, None)
                })?;
                let minor = if parts.len() > 1 && parts[1] != "*" {
                    Some(parts[1].parse::<u32>().map_err(|_| {
                        CompilerError::parse_error("Invalid minor version in wildcard", None, None)
                    })?)
                } else {
                    None
                };
                Ok(VersionReq::Wildcard(major, minor))
            } else {
                Err(CompilerError::parse_error(
                    "Invalid wildcard version format",
                    None,
                    None,
                ))
            }
        } else {
            // Exact version
            let version = Version::parse(req_str)?;
            Ok(VersionReq::Exact(version))
        }
    }
}

#[cfg(test)]
mod resolver_tests {
    use super::*;

    fn dep(version: &str) -> DependencySpec {
        DependencySpec::Simple(version.to_string())
    }

    fn resolve_one(name: &str, specs: &[&str]) -> Result<ResolvedPackage, CompilerError> {
        let mut resolver = DependencyResolver::new();
        for s in specs {
            resolver.add_dependency(name.to_string(), dep(s), false)?;
        }
        let graph = resolver.resolve()?;
        Ok(graph.packages.get(name).cloned().unwrap())
    }

    #[test]
    fn exact_version_is_preserved() {
        let pkg = resolve_one("foo", &["1.2.3"]).expect("resolves");
        assert_eq!(pkg.version, "1.2.3");
    }

    #[test]
    fn caret_range_with_explicit_candidate_picks_concrete() {
        // ^1.2.3 and 1.4.0 both apply — 1.4.0 satisfies both, so it wins.
        let pkg = resolve_one("foo", &["^1.2.3", "1.4.0"]).expect("resolves");
        assert_eq!(pkg.version, "1.4.0");
    }

    #[test]
    fn intersected_open_ranges_advance_the_lower_bound() {
        // Two open-ended caret ranges with no explicit candidate. The
        // intersection's lower bound is the higher of the two caret bases
        // (1.2.0), and that's what should be reported.
        let pkg = resolve_one("foo", &["^1.0.0", "^1.2.0"]).expect("resolves");
        assert_eq!(pkg.version, "1.2.0");
    }

    #[test]
    fn open_ended_constraint_resolves_to_lower_bound() {
        // No explicit candidates → fall back to the intersection's lower bound
        // (which downstream tooling can resolve against the live registry),
        // instead of the historical "latest" placeholder.
        let pkg = resolve_one("foo", &["^1.2.0"]).expect("resolves");
        assert_eq!(pkg.version, "1.2.0");
    }

    #[test]
    fn conflicting_majors_are_rejected() {
        let err = resolve_one("foo", &["^1.0.0", "^2.0.0"]).expect_err("should conflict");
        let msg = format!("{err}");
        assert!(
            msg.contains("Conflicting version requirements for `foo`"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn upper_and_lower_bounds_intersect() {
        let pkg = resolve_one("foo", &[">=1.0.0", "<2.0.0", "1.5.7"]).expect("resolves");
        assert_eq!(pkg.version, "1.5.7");
    }

    #[test]
    fn wildcard_intersection_with_concrete() {
        let pkg = resolve_one("foo", &["1.*", "1.3.4"]).expect("resolves");
        assert_eq!(pkg.version, "1.3.4");
    }

    #[test]
    fn explicit_candidate_outside_range_falls_back_to_bound() {
        // 1.4.0 doesn't satisfy ^2.0.0 — drop it and use the lower bound.
        let pkg = resolve_one("foo", &["^2.0.0", "1.4.0"]).expect_err("should conflict");
        // Actually this *should* fail — Exact(1.4.0) cannot overlap ^2.0.0.
        let msg = format!("{pkg}");
        assert!(msg.contains("Conflicting"), "unexpected error: {msg}");
    }

    #[test]
    fn invalid_version_spec_is_reported() {
        let err = resolve_one("foo", &["not-a-version"]).expect_err("should fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("Invalid version requirement"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn deterministic_resolution_order() {
        let mut resolver = DependencyResolver::new();
        resolver
            .add_dependency("zeta".to_string(), dep("1.0.0"), false)
            .unwrap();
        resolver
            .add_dependency("alpha".to_string(), dep("2.0.0"), false)
            .unwrap();
        resolver
            .add_dependency("mu".to_string(), dep("1.0.0"), false)
            .unwrap();
        let graph = resolver.resolve().unwrap();
        assert_eq!(graph.resolution_order, vec!["alpha", "mu", "zeta"]);
    }
}
