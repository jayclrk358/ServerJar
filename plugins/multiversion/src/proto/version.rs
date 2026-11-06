//! Known Java Edition protocol versions.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version {
    pub protocol: i32,
}

pub struct VersionInfo {
    pub protocol: i32,
    pub name: &'static str,
}

/// Every version this plugin knows how to speak to a client. The newest is the backend
/// (Pumpkin) version; everything else needs a translator in `translate/`.
pub const KNOWN: &[VersionInfo] = &[
    VersionInfo { protocol: 777, name: "26.3" },
    VersionInfo { protocol: 776, name: "26.2" },
    VersionInfo { protocol: 775, name: "26.1" },
    VersionInfo { protocol: 774, name: "1.21.11" },
    VersionInfo { protocol: 773, name: "1.21.9" },
    VersionInfo { protocol: 772, name: "1.21.7" },
    VersionInfo { protocol: 771, name: "1.21.6" },
    VersionInfo { protocol: 770, name: "1.21.5" },
    VersionInfo { protocol: 769, name: "1.21.4" },
    VersionInfo { protocol: 768, name: "1.21.2" },
    VersionInfo { protocol: 767, name: "1.21" },
    VersionInfo { protocol: 766, name: "1.20.5" },
    VersionInfo { protocol: 765, name: "1.20.3" },
    VersionInfo { protocol: 764, name: "1.20.2" },
    VersionInfo { protocol: 763, name: "1.20" },
];

pub const BACKEND: Version = Version { protocol: 777 };

impl Version {
    pub fn name(self) -> String {
        KNOWN
            .iter()
            .find(|v| v.protocol == self.protocol)
            .map_or_else(|| format!("protocol {}", self.protocol), |v| v.name.to_string())
    }
    pub fn from_name(name: &str) -> Option<Version> {
        KNOWN.iter().find(|v| v.name == name).map(|v| Version { protocol: v.protocol })
    }
}
