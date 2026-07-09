use crate::domain::registry::MissionRegistry;
use crate::domain::errors::DomainError;
use crate::proto::{MissionIdentifier, SatelliteIdentifier};

#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub mission: MissionIdentifier,
    pub satellite: SatelliteIdentifier,
    pub specificity_score: u32,
}

pub struct RuleLookupEngine {
    registry: MissionRegistry,
}

impl RuleLookupEngine {
    pub fn new(registry: MissionRegistry) -> Self {
        Self { registry }
    }

    pub fn resolve(
        &self,
        source_id: &str,
        apid: u32,
        vcid: Option<u32>,
    ) -> Result<MatchResult, DomainError> {
        let mut candidates = Vec::new();

        for mission in self.registry.missions() {
            for satellite in &mission.satellites {
                for rule in &satellite.rules {
                    if rule.matches(source_id, apid, vcid) {
                        // Calculate specificity score
                        let mut score = 1; // Base score for source_id match
                        
                        if rule.apids.is_some() {
                            score += 1;
                        }
                        
                        if rule.vcids.is_some() {
                            score += 1;
                        }

                        candidates.push(MatchResult {
                            mission: MissionIdentifier {
                                mission_id: mission.id,
                                mission_name: mission.name.clone(),
                                mission_code: mission.code.clone(),
                            },
                            satellite: SatelliteIdentifier {
                                satellite_id: satellite.id,
                                satellite_name: satellite.name.clone(),
                                norad_id: satellite.norad_id,
                            },
                            specificity_score: score,
                        });
                    }
                }
            }
        }

        if candidates.is_empty() {
            return Err(DomainError::UnidentifiedPacket {
                source_id: source_id.to_string(),
                apid,
                vcid,
            });
        }

        // Sort by specificity score descending
        candidates.sort_by(|a, b| b.specificity_score.cmp(&a.specificity_score));

        // Check for ambiguous matches at the highest score
        if candidates.len() > 1 {
            let best = &candidates[0];
            let next = &candidates[1];
            
            if best.specificity_score == next.specificity_score {
                // If they point to different missions or satellites, it's ambiguous
                if best.mission.mission_id != next.mission.mission_id 
                    || best.satellite.satellite_id != next.satellite.satellite_id 
                {
                    return Err(DomainError::AmbiguousMatch {
                        source_id: source_id.to_string(),
                        apid,
                    });
                }
            }
        }

        Ok(candidates.remove(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_REGISTRY_YAML: &str = r#"
missions:
  - id: 1
    name: "Chandrayaan-3"
    code: "cy3"
    satellites:
      - id: 101
        name: "Propulsion Module"
        norad_id: 57320
        rules:
          - source_id: "rss-replay"
            apids: [42, 43]
          - source_id: "sdr-receiver"
            apids: [10]
            vcids: [1]
      - id: 102
        name: "Lander"
        norad_id: 57321
        rules:
          - source_id: "rss-replay"
            apids: [50]
          - source_id: "sdr-receiver"
            apids: [10]
            vcids: [2]
          - source_id: "ambiguous-source"
            apids: [99]
  - id: 2
    name: "Alternative Mission"
    code: "alt"
    satellites:
      - id: 201
        name: "Alt Sat"
        norad_id: 99999
        rules:
          - source_id: "ambiguous-source"
            apids: [99]
"#;

    #[test]
    fn test_valid_lookup_matching() {
        let registry = MissionRegistry::from_yaml(TEST_REGISTRY_YAML).unwrap();
        let engine = RuleLookupEngine::new(registry);

        // Test basic match Propulsion Module (APID 42)
        let res = engine.resolve("rss-replay", 42, None).unwrap();
        assert_eq!(res.mission.mission_code, "cy3");
        assert_eq!(res.satellite.satellite_id, 101);
        assert_eq!(res.specificity_score, 2);

        // Test basic match Lander (APID 50)
        let res2 = engine.resolve("rss-replay", 50, None).unwrap();
        assert_eq!(res2.mission.mission_code, "cy3");
        assert_eq!(res2.satellite.satellite_id, 102);

        // Test specificity: match source + apid + vcid
        let res3 = engine.resolve("sdr-receiver", 10, Some(1)).unwrap();
        assert_eq!(res3.satellite.satellite_id, 101);
        assert_eq!(res3.specificity_score, 3);

        let res4 = engine.resolve("sdr-receiver", 10, Some(2)).unwrap();
        assert_eq!(res4.satellite.satellite_id, 102);
    }

    #[test]
    fn test_unidentified_packet_error() {
        let registry = MissionRegistry::from_yaml(TEST_REGISTRY_YAML).unwrap();
        let engine = RuleLookupEngine::new(registry);

        // Unregistered APID
        let err = engine.resolve("rss-replay", 999, None).unwrap_err();
        assert_eq!(
            err,
            DomainError::UnidentifiedPacket {
                source_id: "rss-replay".to_string(),
                apid: 999,
                vcid: None
            }
        );

        // Unregistered Source
        let err2 = engine.resolve("unknown-source", 42, None).unwrap_err();
        assert_eq!(
            err2,
            DomainError::UnidentifiedPacket {
                source_id: "unknown-source".to_string(),
                apid: 42,
                vcid: None
            }
        );
    }

    #[test]
    fn test_ambiguous_match_error() {
        let registry = MissionRegistry::from_yaml(TEST_REGISTRY_YAML).unwrap();
        let engine = RuleLookupEngine::new(registry);

        // APID 99 on ambiguous-source matches both mission 1 / satellite 102 AND mission 2 / satellite 201
        let err = engine.resolve("ambiguous-source", 99, None).unwrap_err();
        assert_eq!(
            err,
            DomainError::AmbiguousMatch {
                source_id: "ambiguous-source".to_string(),
                apid: 99
            }
        );
    }
}
