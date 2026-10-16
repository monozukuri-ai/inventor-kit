use super::*;

pub(super) struct Profile(pub Value, pub TargetVersion);
impl Profile {
    pub fn initial_key(&self, record: &Value) -> Result<u16> {
        let owner = string(record, "owner")?;
        let role = string(record, "role")?;
        // Preserve the accepted V61 layout for this fixed box profile. Header
        // keys and record ordinals are separate namespaces; dense allocation
        // does not preserve the profile's implicit registration slots.
        let key = match (owner, role) {
            ("PmAppSegment", "f8a779f1-11d2-098f-c000-5a9a2378d04f/profile-key:1997")
                if self.1 == TargetVersion::Inventor2026 =>
            {
                1996
            }
            ("PmBRepSegment", "geometry_list") => 1,
            ("PmBRepSegment", "feature_map") => 4,
            ("PmBRepSegment", "geometry") => 8,
            ("PmBRepSegment", "carrier") => 2,
            ("PmBRepSegment", "history_helper") => 3,
            ("PmBRepSegment", "empty_list") => 5,
            ("PmBRepSegment", "binding") => 6,
            ("PmBRepSegment", "unit") => 7,
            ("PmAppSegment", "e5dde747-4196-ff57-b19c-fa870db26624/document-id:0") => 1336,
            ("PmAppSegment", "e5dde747-4196-ff57-b19c-fa870db26624/document-id:2") => 1237,
            _ => {
                let (class, key) = role
                    .rsplit_once("/profile-key:")
                    .or_else(|| role.rsplit_once("/key:"))
                    .ok_or_else(|| Error("keyed role has no profile key".into()))?;
                if class != string(record, "class")? {
                    return Err(Error("profile key class differs".into()));
                }
                key.parse::<u16>().map_err(err)?
            }
        };
        if key == 0 || key == u16::MAX {
            return Err(Error("reserved profile key".into()));
        }
        Ok(key)
    }

    #[cfg(test)]
    pub fn load() -> Result<Self> {
        Self::load_for(TargetVersion::default())
    }
    pub fn load_for(target: TargetVersion) -> Result<Self> {
        let source = match target {
            TargetVersion::Inventor2026 => include_str!("profile_2026.json"),
            TargetVersion::Inventor2027 => include_str!("profile.json"),
        };
        let v: Value = serde_json::from_str(source).map_err(err)?;
        if v["schema"] != 1
            || string(&v, "profile")? != target.box_profile()
            || array(&v, "records")?.len() != target.records()
            || array(&v, "owners")?.len() != 8
            || array(&v, "endpoints")?.len() != 39
            || array(&v, "protocols")?.len() != target.protocols()
        {
            return Err(Error("unknown or incomplete writer profile".into()));
        }
        Ok(Self(v, target))
    }
}
