use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Rule {
    pub source_id: String,
    pub apids: Option<Vec<u32>>,
    pub vcids: Option<Vec<u32>>,
}

impl Rule {
    pub fn matches(&self, source_id: &str, apid: u32, vcid: Option<u32>) -> bool {
        if self.source_id != source_id {
            return false;
        }
        
        // Match APID if rules specify APIDs
        if let Some(ref apids) = self.apids {
            if !apids.contains(&apid) {
                return false;
            }
        }
        
        // Match VCID if rules specify VCIDs
        if let Some(ref vcids) = self.vcids {
            match vcid {
                Some(v) => {
                    if !vcids.contains(&v) {
                        return false;
                    }
                }
                None => return false, // Rule requires VCID, but packet doesn't have it
            }
        }
        
        true
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Satellite {
    pub id: u32,
    pub name: String,
    pub norad_id: u32,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Mission {
    pub id: u32,
    pub name: String,
    pub code: String,
    pub satellites: Vec<Satellite>,
}
