//! One immutable typed build policy; JSON is rendered only at external boundaries.
use crate::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};
mod source;
pub(crate) use source::PackageSource;
const CLI_PACKAGE: &str = eml_core::product::CLI_NAME;
const MAX_PLAN_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OsFamily {
    Macos,
    Linux,
    Windows,
}
impl OsFamily {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
        }
    }
    fn suffix(self) -> &'static str {
        match self {
            Self::Macos => "apple-darwin",
            Self::Linux => "unknown-linux-gnu",
            Self::Windows => "pc-windows-gnullvm",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Architecture {
    X86_64,
    Aarch64,
}
impl Architecture {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PlatformId {
    family: OsFamily,
    architecture: Architecture,
}
impl PlatformId {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        let (family, architecture) = value.split_once('-').ok_or("Invalid platform identifier")?;
        Ok(Self {
            family: match family {
                "macos" => OsFamily::Macos,
                "linux" => OsFamily::Linux,
                "windows" => OsFamily::Windows,
                _ => return Err("Unknown platform family".into()),
            },
            architecture: match architecture {
                "x86_64" => Architecture::X86_64,
                "aarch64" => Architecture::Aarch64,
                _ => return Err("Unknown native architecture".into()),
            },
        })
    }
    pub(crate) fn family(self) -> OsFamily {
        self.family
    }
    pub(crate) fn architecture(self) -> Architecture {
        self.architecture
    }
}
impl fmt::Display for PlatformId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.family.as_str(), self.architecture.as_str())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductRole {
    Cli,
    #[serde(rename = "app")]
    DesktopApp,
}
impl ProductRole {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::DesktopApp => "app",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ArtifactKey {
    platform: PlatformId,
    role: ProductRole,
}
impl ArtifactKey {
    pub(crate) fn new(platform: PlatformId, role: ProductRole) -> Self {
        Self { platform, role }
    }
    pub(crate) fn platform(self) -> PlatformId {
        self.platform
    }
    pub(crate) fn role(self) -> ProductRole {
        self.role
    }
}
macro_rules! text_value {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(crate) struct $name(String);
        impl $name {
            pub(crate) fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
text_value!(PackageVersion);
text_value!(RustTarget);
text_value!(RunnerLabel);
text_value!(ArchiveBasename);
text_value!(ArchiveRoot);
text_value!(ArchiveMemberPath);
text_value!(SourceCommit);
impl ArchiveBasename {
    pub(crate) fn parse(value: String) -> Result<Self> {
        if !plain(&value, 255) {
            return Err("Archive name must be one bounded plain native component".into());
        }
        Ok(Self(value))
    }
}
impl SourceCommit {
    fn parse(value: String) -> Result<Self> {
        if value.len() != 40
            || !value
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("Source must be a full lowercase commit identity".into());
        }
        Ok(Self(value))
    }
}
fn plain(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
}
fn runner(value: String) -> Result<RunnerLabel> {
    if !plain(&value, 64) || !value.as_bytes()[0].is_ascii_alphanumeric() {
        return Err("Invalid runner label".into());
    }
    Ok(RunnerLabel(value))
}
fn bounded(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(format!("Input must be an ordinary bounded file: {}", path.display()).into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err("Build input grew beyond its bound".into());
    }
    Ok(bytes)
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlatformFloors {
    macos_minimum: String,
    linux_minimum_ubuntu: String,
    linux_glibc_maximum: String,
    windows_rust_minimum: String,
}
impl PlatformFloors {
    pub(crate) fn macos_minimum(&self) -> &str {
        &self.macos_minimum
    }
    pub(crate) fn linux_minimum_ubuntu(&self) -> &str {
        &self.linux_minimum_ubuntu
    }
    pub(crate) fn linux_glibc_maximum(&self) -> &str {
        &self.linux_glibc_maximum
    }
    pub(crate) fn windows_rust_minimum(&self) -> &str {
        &self.windows_rust_minimum
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct XcodePin {
    version: String,
    build: String,
}
impl XcodePin {
    pub(crate) fn version(&self) -> &str {
        &self.version
    }
    pub(crate) fn build(&self) -> &str {
        &self.build
    }
}
pub(crate) struct ToolPins(BTreeMap<String, String>);
impl ToolPins {
    pub(crate) fn version(&self, name: &str) -> Result<&str> {
        self.0
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("Missing pinned tool: {name}").into())
    }
}
pub(crate) struct PickerCheck {
    runner: RunnerLabel,
    os_major: u32,
}
impl PickerCheck {
    pub(crate) fn runner(&self) -> &str {
        self.runner.as_str()
    }
    pub(crate) fn os_major(&self) -> u32 {
        self.os_major
    }
}
pub(crate) struct DesktopChecks {
    floor_runner: RunnerLabel,
    floor_major: u32,
    picker: Option<PickerCheck>,
}
impl DesktopChecks {
    pub(crate) fn floor_runner(&self) -> &str {
        self.floor_runner.as_str()
    }
    pub(crate) fn floor_major(&self) -> u32 {
        self.floor_major
    }
    pub(crate) fn picker(&self) -> Option<&PickerCheck> {
        self.picker.as_ref()
    }
}
pub(crate) struct NativeTarget {
    platform: PlatformId,
    rust_target: RustTarget,
    build_runner: RunnerLabel,
    products: BTreeSet<ProductRole>,
    desktop: Option<DesktopChecks>,
    platform_text: String,
}
impl NativeTarget {
    pub(crate) fn platform(&self) -> PlatformId {
        self.platform
    }
    pub(crate) fn rust_target(&self) -> &str {
        self.rust_target.as_str()
    }
    pub(crate) fn build_runner(&self) -> &str {
        self.build_runner.as_str()
    }
    pub(crate) fn products(&self) -> &BTreeSet<ProductRole> {
        &self.products
    }
    pub(crate) fn desktop(&self) -> Option<&DesktopChecks> {
        self.desktop.as_ref()
    }
    pub(crate) fn platform_text(&self) -> &str {
        &self.platform_text
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArchiveFormat {
    TarGz,
    Zip,
}
#[derive(Clone, Copy)]
pub(crate) enum AuxiliaryAsset {
    ChecksumManifest,
}
impl AuxiliaryAsset {
    pub(crate) fn basename(self) -> &'static str {
        match self {
            Self::ChecksumManifest => "SHA256SUMS",
        }
    }
}
pub(crate) struct ArtifactSpec {
    key: ArtifactKey,
    basename: ArchiveBasename,
    root: ArchiveRoot,
    executable: ArchiveMemberPath,
    format: ArchiveFormat,
}
impl ArtifactSpec {
    pub(crate) fn key(&self) -> ArtifactKey {
        self.key
    }
    pub(crate) fn basename(&self) -> &str {
        self.basename.as_str()
    }
    pub(crate) fn root(&self) -> &str {
        self.root.as_str()
    }
    pub(crate) fn executable(&self) -> &str {
        self.executable.as_str()
    }
    pub(crate) fn format(&self) -> ArchiveFormat {
        self.format
    }
}
pub(crate) struct BuildPlan {
    source: PackageSource,
    version: PackageVersion,
    floors: PlatformFloors,
    targets: BTreeMap<PlatformId, NativeTarget>,
    artifacts: BTreeMap<ArtifactKey, ArtifactSpec>,
    tools: ToolPins,
    xcode: XcodePin,
}
pub(crate) struct BuildContext {
    root: PathBuf,
    plan: BuildPlan,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Distribution {
    policy: PlatformFloors,
    targets: Vec<TargetInput>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetInput {
    platform: String,
    target: String,
    runner: String,
    products: Vec<ProductRole>,
    desktop: Option<DesktopInput>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopInput {
    floor_runner: String,
    floor_major: u32,
    picker: Option<PickerInput>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PickerInput {
    runner: String,
    os_major: u32,
}
#[derive(Deserialize)]
struct ToolInput {
    xcode: XcodePin,
    versions: BTreeMap<String, String>,
}
impl BuildPlan {
    #[cfg(test)]
    pub(crate) fn test_plan() -> Self {
        Self::parse(
            include_str!("../distribution.toml"),
            include_str!("../tools.toml"),
            env!("CARGO_PKG_VERSION").into(),
            PackageSource::fixture(),
        )
        .unwrap()
    }

    pub(crate) fn parse(
        distribution: &str,
        tool_input: &str,
        version: String,
        source: PackageSource,
    ) -> Result<Self> {
        let table: Distribution = toml::from_str(distribution)?;
        let tools: ToolInput = toml::from_str(tool_input)?;
        if !plain(&version, 128) {
            return Err("Cargo version cannot form a safe archive component".into());
        }
        for value in [
            &table.policy.macos_minimum,
            &table.policy.linux_minimum_ubuntu,
            &table.policy.linux_glibc_maximum,
            &table.policy.windows_rust_minimum,
            &tools.xcode.version,
        ] {
            let parts: Vec<_> = value.split('.').collect();
            if value.len() > 16
                || parts.is_empty()
                || parts.len() > 3
                || parts
                    .iter()
                    .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
                || parts[0].bytes().all(|b| b == b'0')
            {
                return Err("Invalid platform floor or Xcode version".into());
            }
        }
        if tools.xcode.version.split('.').count() < 2
            || !plain(&tools.xcode.build, 32)
            || !tools.xcode.build.bytes().all(|b| b.is_ascii_alphanumeric())
            || tools.versions.is_empty()
            || tools.versions.iter().any(|(name, version)| {
                name.is_empty()
                    || name.len() > 64
                    || !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                    || !plain(version, 64)
            })
        {
            return Err("Invalid compiler-adjacent tool pin".into());
        }
        let mut targets = BTreeMap::new();
        let mut artifacts = BTreeMap::new();
        let mut names = BTreeSet::new();
        let mut rust_targets = BTreeSet::new();
        for input in table.targets {
            let platform = PlatformId::parse(&input.platform)?;
            if input.target
                != format!(
                    "{}-{}",
                    platform.architecture.as_str(),
                    platform.family.suffix()
                )
                || !rust_targets.insert(input.target.clone())
            {
                return Err("Duplicate or platform-mismatched Rust target".into());
            }
            let products: BTreeSet<_> = input.products.iter().copied().collect();
            if products.is_empty()
                || products.len() != input.products.len()
                || (products.contains(&ProductRole::DesktopApp) != input.desktop.is_some())
                || (input.desktop.is_some() && platform.family != OsFamily::Macos)
            {
                return Err("Invalid or duplicate product/desktop roles".into());
            }
            let desktop = input
                .desktop
                .map(|desktop| -> Result<DesktopChecks> {
                    if !(1..=100).contains(&desktop.floor_major) {
                        return Err("Invalid desktop floor major".into());
                    }
                    Ok(DesktopChecks {
                        floor_runner: runner(desktop.floor_runner)?,
                        floor_major: desktop.floor_major,
                        picker: desktop
                            .picker
                            .map(|picker| -> Result<PickerCheck> {
                                if picker.os_major < desktop.floor_major || picker.os_major > 100 {
                                    return Err("Invalid picker OS role".into());
                                }
                                Ok(PickerCheck {
                                    runner: runner(picker.runner)?,
                                    os_major: picker.os_major,
                                })
                            })
                            .transpose()?,
                    })
                })
                .transpose()?;
            let build_runner = runner(input.runner)?;
            let platform_text = match platform.family {
                OsFamily::Macos => format!(
                    "macOS {}+, {}. Native build host {}; complete desktop floor host {}. A deployment minimum is not execution evidence for the original OS release.",
                    table.policy.macos_minimum,
                    platform.architecture.as_str(),
                    build_runner.as_str(),
                    desktop.as_ref().map_or("none", DesktopChecks::floor_runner)
                ),
                OsFamily::Linux => format!(
                    "Ubuntu {}+, GNU/glibc {}; required GLIBC symbols no newer than {}. Native qualification required on {}; other distributions are unqualified.",
                    table.policy.linux_minimum_ubuntu,
                    platform.architecture.as_str(),
                    table.policy.linux_glibc_maximum,
                    build_runner.as_str()
                ),
                OsFamily::Windows => format!(
                    "Rust target minimum Windows {}+, {}. Native qualification required on {}; client patch coverage depends on actual qualification records.",
                    table.policy.windows_rust_minimum,
                    platform.architecture.as_str(),
                    build_runner.as_str()
                ),
            };
            for &role in &products {
                let key = ArtifactKey { platform, role };
                let (basename, root, executable, format) = match role {
                    ProductRole::Cli => {
                        let root = format!("{CLI_PACKAGE}-{version}-{platform}");
                        let format = if platform.family != OsFamily::Windows {
                            ArchiveFormat::TarGz
                        } else {
                            ArchiveFormat::Zip
                        };
                        (
                            format!(
                                "{root}.{}",
                                if format == ArchiveFormat::TarGz {
                                    "tar.gz"
                                } else {
                                    "zip"
                                }
                            ),
                            root,
                            if platform.family == OsFamily::Windows {
                                "emlattachmentremover.exe"
                            } else {
                                CLI_PACKAGE
                            }
                            .to_owned(),
                            format,
                        )
                    }
                    ProductRole::DesktopApp => (
                        format!("emlattachmentremover-gui-{version}-{platform}.zip"),
                        "EML Attachment Remover.app".into(),
                        "Contents/MacOS/EMLAttachmentRemover".into(),
                        ArchiveFormat::Zip,
                    ),
                };
                if !plain(&basename, 255) || !names.insert(basename.clone()) {
                    return Err("Archive basenames must be unique bounded components".into());
                }
                artifacts.insert(
                    key,
                    ArtifactSpec {
                        key,
                        basename: ArchiveBasename::parse(basename)?,
                        root: ArchiveRoot(root),
                        executable: ArchiveMemberPath(executable),
                        format,
                    },
                );
            }
            let row = NativeTarget {
                platform,
                rust_target: RustTarget(input.target),
                build_runner,
                products,
                desktop,
                platform_text,
            };
            if targets.insert(platform, row).is_some() {
                return Err("Duplicate native platform".into());
            }
        }
        if targets.is_empty() || artifacts.is_empty() {
            return Err("Distribution has no native products".into());
        }
        Ok(Self {
            source,
            version: PackageVersion(version),
            floors: table.policy,
            targets,
            artifacts,
            tools: ToolPins(tools.versions),
            xcode: tools.xcode,
        })
    }
    pub(crate) fn source_commit(&self) -> &str {
        self.source.commit()
    }
    pub(crate) fn source_epoch(&self) -> xtask::archive::PackageEpoch {
        self.source.epoch()
    }
    pub(crate) fn source_provenance(&self) -> &str {
        self.source.provenance()
    }
    pub(crate) fn source_working_tree(&self, root: &Path) -> Result<Option<String>> {
        self.source.working_tree(root)
    }
    pub(crate) fn verify_packaging_source(&self, root: &Path) -> Result<()> {
        self.source.verify_packaging_source(root)
    }
    pub(crate) fn require_qualified_source(&self, root: &Path) -> Result<()> {
        self.source.require_qualified_source(root)
    }
    pub(crate) fn version(&self) -> &str {
        self.version.as_str()
    }
    pub(crate) fn floors(&self) -> &PlatformFloors {
        &self.floors
    }
    pub(crate) fn tools(&self) -> &ToolPins {
        &self.tools
    }
    pub(crate) fn xcode(&self) -> &XcodePin {
        &self.xcode
    }
    pub(crate) fn targets(&self) -> impl Iterator<Item = &NativeTarget> {
        self.targets.values()
    }
    pub(crate) fn expected_artifacts(&self) -> impl Iterator<Item = &ArtifactSpec> {
        self.artifacts.values()
    }
    pub(crate) fn target(&self, key: PlatformId) -> &NativeTarget {
        self.targets
            .get(&key)
            .expect("internally admitted platform key")
    }
    pub(crate) fn lookup_target(&self, target: &str) -> Result<&NativeTarget> {
        self.targets
            .values()
            .find(|row| row.rust_target() == target)
            .ok_or_else(|| format!("Unsupported native Rust target: {target}").into())
    }
    pub(crate) fn lookup_platform(&self, platform: &str) -> Result<&NativeTarget> {
        let id = PlatformId::parse(platform)?;
        if self.targets.contains_key(&id) {
            Ok(self.target(id))
        } else {
            Err(format!("Undeclared native platform: {platform}").into())
        }
    }
    pub(crate) fn artifact(&self, key: ArtifactKey) -> &ArtifactSpec {
        self.artifacts
            .get(&key)
            .expect("internally admitted artifact key")
    }
    pub(crate) fn lookup_artifact(
        &self,
        platform: PlatformId,
        role: ProductRole,
    ) -> Result<&ArtifactSpec> {
        let key = ArtifactKey::new(platform, role);
        if self.artifacts.contains_key(&key) {
            Ok(self.artifact(key))
        } else {
            Err("Target does not declare this product role".into())
        }
    }
    pub(crate) fn named_artifact(&self, name: &str) -> Result<&ArtifactSpec> {
        self.artifacts
            .values()
            .find(|row| row.basename() == name)
            .ok_or_else(|| "Undeclared archive basename/version/platform".into())
    }
    pub(crate) fn archive_names(&self) -> BTreeSet<String> {
        self.expected_artifacts()
            .map(|row| row.basename().to_owned())
            .collect()
    }
    fn row_json(row: &NativeTarget) -> Value {
        json!({"platform":row.platform.to_string(),"target":row.rust_target(),"runner":row.build_runner(),"app_smoke_runner":row.desktop().map(DesktopChecks::floor_runner),"os":row.platform.family.as_str(),"architecture":row.platform.architecture.as_str(),"app":row.products.contains(&ProductRole::DesktopApp),"products":row.products.iter().map(|role|role.as_str()).collect::<Vec<_>>(),"platform_text":row.platform_text()})
    }
    pub(crate) fn native_matrix(&self) -> Value {
        json!({"include":self.targets().map(Self::row_json).collect::<Vec<_>>()})
    }
    pub(crate) fn app_matrix(&self) -> Value {
        json!({"include":self.targets().filter(|row|row.products.contains(&ProductRole::DesktopApp)).map(Self::row_json).collect::<Vec<_>>()})
    }
    pub(crate) fn floor_matrix(&self) -> Value {
        json!({"include":self.targets().filter_map(|row|row.desktop().map(|desktop|json!({"platform":row.platform.to_string(),"target":row.rust_target(),"runner":desktop.floor_runner(),"os_major":desktop.floor_major()}))).collect::<Vec<_>>()})
    }
    pub(crate) fn picker_matrix(&self) -> Value {
        json!({"include":self.targets().filter_map(|row|row.desktop().and_then(DesktopChecks::picker).map(|picker|json!({"platform":row.platform.to_string(),"target":row.rust_target(),"runner":picker.runner(),"os_major":picker.os_major()}))).collect::<Vec<_>>()})
    }
    pub(crate) fn render(&self) -> Value {
        json!({"schema":"eml-build-plan","schema_version":1,"source":{"commit":self.source_commit(),"epoch":self.source_epoch(),"provenance":self.source_provenance()},"version":self.version(),"policy":{"macos_minimum":self.floors.macos_minimum(),"linux_minimum_ubuntu":self.floors.linux_minimum_ubuntu(),"linux_glibc_maximum":self.floors.linux_glibc_maximum(),"windows_rust_minimum":self.floors.windows_rust_minimum()},"native_targets":self.targets().map(Self::row_json).collect::<Vec<_>>(),"expected_artifacts":self.expected_artifacts().map(|row|json!({"name":row.basename(),"platform":row.key.platform.to_string(),"role":row.key.role.as_str(),"root":row.root(),"executable":row.executable(),"format":match row.format() {ArchiveFormat::Zip=>"zip",ArchiveFormat::TarGz=>"tar.gz"}})).collect::<Vec<_>>(),"xcode":{"version":self.xcode().version(),"build":self.xcode().build()},"tools":self.tools.0,"native_matrix":self.native_matrix(),"app_matrix":self.app_matrix(),"floor_matrix":self.floor_matrix(),"picker_matrix":self.picker_matrix()})
    }
}
impl BuildContext {
    pub(crate) fn load(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let metadata = Command::new(cargo)
            .current_dir(&root)
            .args([
                "metadata",
                "--locked",
                "--offline",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(root.join("Cargo.toml"))
            .output()?;
        if !metadata.status.success() {
            std::io::stderr().write_all(&metadata.stderr)?;
            return Err(format!("Cargo metadata failed: {}", metadata.status).into());
        }
        if metadata.stdout.len() > 8 * 1024 * 1024 {
            return Err("Cargo metadata exceeds its input bound".into());
        }
        let version = workspace_version(&root, &metadata.stdout)?;
        let source = PackageSource::observe(&root)?;
        let plan = BuildPlan::parse(
            std::str::from_utf8(&bounded(
                &root.join("xtask/distribution.toml"),
                MAX_PLAN_BYTES,
            )?)?,
            std::str::from_utf8(&bounded(&root.join("xtask/tools.toml"), MAX_PLAN_BYTES)?)?,
            version,
            source,
        )?;
        Ok(Self { root, plan })
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn source(&self) -> &str {
        self.plan.source_commit()
    }
    pub(crate) fn plan(&self) -> &BuildPlan {
        &self.plan
    }
}
pub(crate) fn run(context: &BuildContext, github_output: Option<&Path>) -> Result<()> {
    let plan = context.plan();
    let mut bytes = serde_json::to_vec(&plan.render())?;
    bytes.push(b'\n');
    if bytes.len() > MAX_PLAN_BYTES {
        return Err("Distribution output exceeds its bound".into());
    }
    if let Some(path) = github_output {
        let mut fields = vec![
            (
                "source_date_epoch".to_owned(),
                json!(plan.source_epoch().seconds()),
            ),
            ("version".to_owned(), json!(plan.version())),
            ("native".into(), plan.native_matrix()),
            ("apps".into(), plan.app_matrix()),
            ("floors".into(), plan.floor_matrix()),
            ("picker".into(), plan.picker_matrix()),
            ("xcode".into(), json!(plan.xcode().version())),
            ("xcode_build".into(), json!(plan.xcode().build())),
            ("minimum".into(), json!(plan.floors.macos_minimum())),
        ];
        fields.extend(
            plan.tools
                .0
                .iter()
                .map(|(name, value)| (name.clone(), json!(value))),
        );
        let mut names = BTreeSet::new();
        let mut encoded = String::new();
        for (name, value) in fields {
            if !names.insert(name.clone()) {
                return Err("Developer tool pin collides with a workflow output field".into());
            }
            let text = if let Some(value) = value.as_str() {
                value.to_owned()
            } else {
                serde_json::to_string(&value)?
            };
            if text.len() > MAX_PLAN_BYTES || text.contains(['\r', '\n']) {
                return Err("GitHub output must be a bounded single line".into());
            }
            use std::fmt::Write as _;
            writeln!(encoded, "{name}={text}")?;
        }
        if encoded.len() > 4 * MAX_PLAN_BYTES {
            return Err("GitHub workflow outputs exceed their total bound".into());
        }
        let mut output = std::fs::OpenOptions::new().append(true).open(path)?;
        output.write_all(encoded.as_bytes())?;
        output.flush()?;
    }
    std::io::stdout().lock().write_all(&bytes)?;
    Ok(())
}
#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
}
#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    manifest_path: PathBuf,
    #[serde(deserialize_with = "required_source")]
    source: Option<String>,
}
fn required_source<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::deserialize(input)
}
fn workspace_version(root: &Path, bytes: &[u8]) -> Result<String> {
    let document: serde_json::Value = serde_json::from_slice(bytes)?;
    if !document.is_object()
        || !document
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|packages| packages.iter().all(serde_json::Value::is_object))
    {
        return Err("Cargo metadata requires workspace and package objects".into());
    }
    let metadata: Metadata = serde_json::from_value(document)?;
    let root = root.canonicalize()?;
    if metadata.workspace_root.canonicalize()? != root {
        return Err("Cargo metadata refers to another workspace".into());
    }
    let mut candidates = metadata
        .packages
        .into_iter()
        .filter(|package| package.name == CLI_PACKAGE);
    let package = candidates
        .next()
        .ok_or("Workspace CLI package missing from Cargo metadata")?;
    if candidates.next().is_some()
        || package.id.is_empty()
        || metadata
            .workspace_members
            .iter()
            .filter(|id| **id == package.id)
            .count()
            != 1
        || package.source.is_some()
        || package.manifest_path.canonicalize()?
            != root.join("crates/eml-cli/Cargo.toml").canonicalize()?
    {
        return Err("Cargo CLI package identity does not match this workspace source".into());
    }
    Ok(package.version)
}

#[cfg(test)]
mod tests;
