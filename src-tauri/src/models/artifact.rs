use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArtifactPlatform {
    Windows,
    Linux,
}

impl ArtifactPlatform {
    pub(crate) fn host() -> Result<Self, String> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => Ok(Self::Windows),
            ("linux", "x86_64") => Ok(Self::Linux),
            _ => Err("Official server artifacts require Windows or Linux on x86_64.".into()),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }

    pub(crate) fn legacy_base(self) -> &'static str {
        match self {
            Self::Windows => {
                "https://runtime.fivem.net/artifacts/fivem/build_server_windows/master/"
            }
            Self::Linux => "https://runtime.fivem.net/artifacts/fivem/build_proot_linux/master/",
        }
    }

    pub(crate) fn legacy_archive(self) -> &'static str {
        match self {
            Self::Windows => "server.zip",
            Self::Linux => "fx.tar.xz",
        }
    }

    pub(crate) fn enhanced_archive(self) -> &'static str {
        match self {
            Self::Windows => "cfx-server_win_x64.zip",
            Self::Linux => "cfx-server_linux_x64.tar.xz",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactIssue {
    pub artifact: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMetadata {
    pub recommended_artifact: String,
    #[serde(default)]
    pub windows_download_link: String,
    #[serde(default)]
    pub linux_download_link: Option<String>,
    #[serde(default)]
    pub broken_artifacts: Vec<ArtifactIssue>,
}

impl ArtifactMetadata {
    pub(crate) fn download_link(&self, platform: ArtifactPlatform) -> Option<&str> {
        match platform {
            ArtifactPlatform::Windows => Some(self.windows_download_link.as_str()),
            ArtifactPlatform::Linux => self.linux_download_link.as_deref(),
        }
        .map(str::trim)
        .filter(|link| !link.is_empty())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactInstallRequest {
    pub version: String,
    pub url: String,
    pub destination: String,
    #[serde(default)]
    pub acknowledge_risk: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactBuild {
    pub version: String,
    pub download_url: String,
    pub health: String,
    pub issues: Vec<ArtifactIssue>,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactCatalog {
    pub builds: Vec<ArtifactBuild>,
    pub fetched_at: u64,
    pub metadata_fetched_at: Option<u64>,
    pub stale: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactInstallResult {
    pub version: String,
    pub destination: String,
    pub marker_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledArtifactInfo {
    pub installed: bool,
    pub version: Option<String>,
    pub destination: String,
    pub marker_path: String,
    pub citizen_server_impl_path: Option<String>,
    pub file_version: Option<String>,
    pub product_version: Option<String>,
    pub has_fxserver_executable: bool,
    pub edition: Option<String>,
    pub detection_source: String,
}

// Enhanced wire types adapted from Huntercorlett's fork (53f1834).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnhancedArtifactBuild {
    pub version: String,
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnhancedArtifactCatalog {
    pub builds: Vec<EnhancedArtifactBuild>,
    pub source_url: String,
    pub warning: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnhancedInstallRequest {
    pub url: String,
    pub destination: String,
}
