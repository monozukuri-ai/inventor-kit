//! Exact output versions, independent of the installed Inventor environment.
use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetVersion {
    Inventor2026,
    #[default]
    Inventor2027,
}
impl TargetVersion {
    pub fn year(self) -> u16 {
        match self {
            Self::Inventor2026 => 2026,
            Self::Inventor2027 => 2027,
        }
    }
    pub(super) fn major(self) -> u8 {
        (self.year() - 1996) as u8
    }
    pub(super) fn from_major(major: u8) -> Result<Self> {
        match major {
            30 => Ok(Self::Inventor2026),
            31 => Ok(Self::Inventor2027),
            _ => Err(Error("unsupported Inventor target version".into())),
        }
    }
    pub(super) fn creation(self) -> [u16; 4] {
        match self {
            Self::Inventor2026 => [40, 16414, 0, 43500],
            Self::Inventor2027 => [10, 16415, 1, 27000],
        }
    }
    pub(super) fn save(self) -> [u16; 4] {
        match self {
            Self::Inventor2026 => [0, 16414, 0, 13000],
            Self::Inventor2027 => [0, 16415, 0, 15500],
        }
    }
    pub(super) fn protocols(self) -> usize {
        match self {
            Self::Inventor2026 => 560,
            Self::Inventor2027 => 564,
        }
    }
    pub(super) fn records(self) -> usize {
        match self {
            Self::Inventor2026 => 788,
            Self::Inventor2027 => 789,
        }
    }
    pub(super) fn box_profile(self) -> &'static str {
        match self {
            Self::Inventor2026 => PROFILE_ID_2026,
            Self::Inventor2027 => PROFILE_ID,
        }
    }
    pub(super) fn asm_version(self) -> &'static str {
        match self {
            Self::Inventor2026 => "231.6.4.65535",
            Self::Inventor2027 => "232.6.0.65535",
        }
    }
    pub(super) fn sab_version(self) -> u32 {
        match self {
            Self::Inventor2026 => 23100,
            Self::Inventor2027 => 23200,
        }
    }
}
