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
    #[allow(dead_code)]
    registries: Vec<String>,
    /// Module resolver for loading packages
    #[allow(dead_code)]
    module_resolver: ModuleResolver,
    /// Installed packages cache
    #[allow(dead_code)]
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
                &format!("Failed to read package manifest: {e}"),
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
                CompilerError::io_error(&format!("Failed to serialize manifest: {e}"), None, None)
            })?
        } else {
            toml::to_string_pretty(manifest).map_err(|e| {
                CompilerError::io_error(&format!("Failed to serialize manifest: {e}"), None, None)
            })?
        };

        fs::write(&path, content).map_err(|e| {
            CompilerError::io_error(
                &format!("Failed to write package manifest: {e}"),
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
            CompilerError::io_error(&format!("Failed to create src directory: {e}"), None, None)
        })?;

        // Create main.clean file
        let main_file = src_dir.join("main.clean");
        if !main_file.exists() {
            let main_content = format!(
                "// {name} - Clean Language Package\n\nfunction start()\n\tprint(\"Hello from {name}!\")\n"
            );
            fs::write(&main_file, main_content).map_err(|e| {
                CompilerError::io_error(&format!("Failed to create main.clean: {e}"), None, None)
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
                &format!("Failed to create install directory: {e}"),
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
                &format!("Failed to create install directory: {e}"),
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
                &format!("Failed to create install directory: {e}"),
                None,
                None,
            )
        })?;

        println!(
            "📁 Copying from local path: {path}",
            path = source_path.display()
        );

        // Copy package files to install location
        self.copy_dir_recursive(source_path, install_path)?;

        Ok(())
    }

    /// Recursively copy directory
    fn copy_dir_recursive(&self, src: &Path, dst: &Path) -> Result<(), CompilerError> {
        if !dst.exists() {
            fs::create_dir_all(dst).map_err(|e| {
                CompilerError::io_error(&format!("Failed to create directory: {e}"), None, None)
            })?;
        }

        for entry in fs::read_dir(src).map_err(|e| {
            CompilerError::io_error(&format!("Failed to read directory: {e}"), None, None)
        })? {
            let entry = entry.map_err(|e| {
                CompilerError::io_error(&format!("Failed to read directory entry: {e}"), None, None)
            })?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if src_path.is_dir() {
                self.copy_dir_recursive(&src_path, &dst_path)?;
            } else {
                fs::copy(&src_path, &dst_path).map_err(|e| {
                    CompilerError::io_error(&format!("Failed to copy file: {e}"), None, None)
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
        // Simplified dependency resolution
        // In a real implementation, this would handle version constraints,
        // conflict resolution, and transitive dependencies

        let mut packages = HashMap::new();
        let mut resolution_order = Vec::new();

        for (name, specs) in &self.dependencies {
            // For now, just take the first specification
            if let Some((spec, _)) = specs.first() {
                let version = match spec {
                    DependencySpec::Simple(v) => v.clone(),
                    DependencySpec::Detailed {
                        version: Some(v), ..
                    } => v.clone(),
                    _ => "latest".to_string(),
                };

                let resolved_package = ResolvedPackage {
                    name: name.clone(),
                    version: version.clone(),
                    source: PackageSource::Registry {
                        url: "https://packages.cleanlang.org".to_string(),
                    },
                    dependencies: HashMap::new(),
                };

                packages.insert(name.clone(), resolved_package);
                resolution_order.push(name.clone());
            }
        }

        Ok(DependencyGraph {
            packages,
            resolution_order,
        })
    }
}

impl Version {
    /// Parse version string
    pub fn parse(version_str: &str) -> Result<Version, CompilerError> {
        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() < 3 {
            return Err(CompilerError::parse_error(
                &format!("Invalid version format: {version_str}"),
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
                self.major == *major && minor.map_or(true, |m| self.minor == m)
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

        if req_str.starts_with('^') {
            let version = Version::parse(&req_str[1..])?;
            Ok(VersionReq::Caret(version))
        } else if req_str.starts_with('~') {
            let version = Version::parse(&req_str[1..])?;
            Ok(VersionReq::Tilde(version))
        } else if req_str.starts_with(">=") {
            let version = Version::parse(&req_str[2..])?;
            Ok(VersionReq::GreaterEqual(version))
        } else if req_str.starts_with('>') {
            let version = Version::parse(&req_str[1..])?;
            Ok(VersionReq::GreaterThan(version))
        } else if req_str.starts_with("<=") {
            let version = Version::parse(&req_str[2..])?;
            Ok(VersionReq::LessEqual(version))
        } else if req_str.starts_with('<') {
            let version = Version::parse(&req_str[1..])?;
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
