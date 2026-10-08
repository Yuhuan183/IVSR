//! Choosing the release asset that fits the running platform.

use serde::Serialize;

use crate::{Asset, Release};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Macos,
    Linux,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    X64,
    Arm64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    pub fn current() -> Self {
        let os = if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        };
        let arch = if cfg!(target_arch = "aarch64") { Arch::Arm64 } else { Arch::X64 };
        Self { os, arch }
    }

    pub fn os_key(&self) -> &'static str {
        match self.os {
            Os::Macos => "macos",
            Os::Linux => "linux",
            Os::Windows => "windows",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let arch = match self.arch {
            Arch::X64 => "x64",
            Arch::Arm64 => "arm64",
        };
        write!(f, "{}-{arch}", self.os_key())
    }
}

pub trait AssetSelector: Send + Sync {
    fn select<'a>(&self, release: &'a Release) -> Option<&'a Asset>;
}

/// Matches assets by OS/arch tokens in their names, e.g.
/// `ivsr-0.2.0-aarch64-apple-darwin.tar.gz`, `ivsr_0.2.0_x64-setup.exe`,
/// `realesrgan-ncnn-vulkan-20220424-macos.zip`.
#[derive(Debug, Clone)]
pub struct PlatformSelector {
    pub platform: Platform,
    /// Token that must appear in the name (e.g. `cli`), if any.
    pub require: Option<String>,
    /// Name suffixes in order of preference (e.g. `.tar.gz`, `.zip`); assets
    /// matching none are rejected when the list is non-empty.
    pub suffixes: Vec<String>,
}

impl PlatformSelector {
    pub fn new(platform: Platform) -> Self {
        Self { platform, require: None, suffixes: Vec::new() }
    }

    pub fn require(mut self, token: impl Into<String>) -> Self {
        self.require = Some(token.into().to_ascii_lowercase());
        self
    }

    pub fn suffixes<S: Into<String>>(mut self, suffixes: impl IntoIterator<Item = S>) -> Self {
        self.suffixes = suffixes.into_iter().map(|s| s.into().to_ascii_lowercase()).collect();
        self
    }

    fn score(&self, asset: &Asset) -> Option<i32> {
        let name = asset.name.to_ascii_lowercase();
        if [".sha256", ".sha512", ".sig", ".asc", ".txt", ".json", ".pdb"].iter().any(|s| name.ends_with(s)) {
            return None;
        }
        let normalised = name.replace("x86_64", "x64").replace("amd64", "x64").replace("aarch64", "arm64");
        let tokens: Vec<&str> = normalised.split(|c: char| !c.is_ascii_alphanumeric()).filter(|t| !t.is_empty()).collect();
        let has = |words: &[&str]| tokens.iter().any(|t| words.contains(t));

        let os_of = |os: Os| match os {
            Os::Macos => has(&["macos", "darwin", "apple", "osx", "mac", "dmg"]),
            Os::Linux => has(&["linux", "ubuntu", "appimage", "deb", "rpm"]),
            Os::Windows => has(&["windows", "win", "win64", "msvc", "msi", "exe"]),
        };
        if !os_of(self.platform.os) {
            return None;
        }
        let mut score = 0;
        match (has(&["x64"]), has(&["arm64"]), has(&["universal"])) {
            (_, _, true) => score += 1,
            (true, false, _) if self.platform.arch != Arch::X64 => return None,
            (false, true, _) if self.platform.arch != Arch::Arm64 => return None,
            (false, false, _) => {}
            _ => score += 2,
        }
        if let Some(token) = &self.require
            && !tokens.contains(&token.as_str()) {
                return None;
            }
        if !self.suffixes.is_empty() {
            let rank = self.suffixes.iter().position(|s| name.ends_with(s.as_str()))?;
            score += 10 * (self.suffixes.len() - rank) as i32;
        }
        Some(score)
    }
}

impl AssetSelector for PlatformSelector {
    fn select<'a>(&self, release: &'a Release) -> Option<&'a Asset> {
        release
            .assets
            .iter()
            .filter_map(|a| self.score(a).map(|s| (s, a)))
            .max_by_key(|(s, _)| *s)
            .map(|(_, a)| a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(names: &[&str]) -> Release {
        Release {
            tag: "v1.0.0".into(),
            version: None,
            name: String::new(),
            notes: String::new(),
            prerelease: false,
            published_at: None,
            page_url: None,
            assets: names
                .iter()
                .map(|n| Asset { name: n.to_string(), size: 1, download_url: String::new(), api_url: None, digest: None })
                .collect(),
        }
    }

    fn pick<'a>(sel: &PlatformSelector, r: &'a Release) -> Option<&'a str> {
        sel.select(r).map(|a| a.name.as_str())
    }

    const MAC_ARM: Platform = Platform { os: Os::Macos, arch: Arch::Arm64 };
    const WIN_X64: Platform = Platform { os: Os::Windows, arch: Arch::X64 };
    const LINUX_X64: Platform = Platform { os: Os::Linux, arch: Arch::X64 };

    #[test]
    fn realesrgan_assets_match_by_os_word() {
        let r = release(&[
            "realesr-animevideov3.pth",
            "realesrgan-ncnn-vulkan-20220424-macos.zip",
            "realesrgan-ncnn-vulkan-20220424-ubuntu.zip",
            "realesrgan-ncnn-vulkan-20220424-windows.zip",
        ]);
        let zip = |p| PlatformSelector::new(p).suffixes([".zip"]);
        assert_eq!(pick(&zip(MAC_ARM), &r), Some("realesrgan-ncnn-vulkan-20220424-macos.zip"));
        assert_eq!(pick(&zip(LINUX_X64), &r), Some("realesrgan-ncnn-vulkan-20220424-ubuntu.zip"));
        assert_eq!(pick(&zip(WIN_X64), &r), Some("realesrgan-ncnn-vulkan-20220424-windows.zip"));
    }

    #[test]
    fn arch_specific_asset_beats_wrong_arch_and_darwin_is_not_windows() {
        let r = release(&[
            "ivsr-cli-0.2.0-x86_64-apple-darwin.tar.gz",
            "ivsr-cli-0.2.0-aarch64-apple-darwin.tar.gz",
            "ivsr-cli-0.2.0-x86_64-pc-windows-msvc.zip",
            "ivsr-cli-0.2.0-aarch64-apple-darwin.tar.gz.sha256",
        ]);
        let cli = |p| PlatformSelector::new(p).require("cli").suffixes([".tar.gz", ".zip"]);
        assert_eq!(pick(&cli(MAC_ARM), &r), Some("ivsr-cli-0.2.0-aarch64-apple-darwin.tar.gz"));
        assert_eq!(pick(&cli(WIN_X64), &r), Some("ivsr-cli-0.2.0-x86_64-pc-windows-msvc.zip"));
        assert_eq!(pick(&cli(LINUX_X64), &r), None);
    }

    #[test]
    fn desktop_bundles_are_chosen_by_installer_suffix() {
        let r = release(&[
            "ivsr_0.2.0_aarch64.dmg",
            "ivsr_0.2.0_x64.dmg",
            "ivsr_0.2.0_x64-setup.exe",
            "ivsr_0.2.0_x64_en-US.msi",
            "ivsr_0.2.0_amd64.AppImage",
            "ivsr-cli-0.2.0-aarch64-apple-darwin.tar.gz",
        ]);
        let gui = |p| PlatformSelector::new(p).suffixes([".dmg", "-setup.exe", ".msi", ".appimage"]);
        assert_eq!(pick(&gui(MAC_ARM), &r), Some("ivsr_0.2.0_aarch64.dmg"));
        assert_eq!(pick(&gui(WIN_X64), &r), Some("ivsr_0.2.0_x64-setup.exe"));
        assert_eq!(pick(&gui(LINUX_X64), &r), Some("ivsr_0.2.0_amd64.AppImage"));
    }
}
