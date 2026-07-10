use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::path::Path;
use crate::domain::errors::XtceError;
use crate::domain::models::*;

pub struct XtceRegistry {
    pub(crate) cache: RwLock<HashMap<String, Arc<XtceDb>>>,
    db_dir: String,
}

impl XtceRegistry {
    pub fn new(db_dir: String) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            db_dir,
        }
    }

    /// Retrieve the parsed database from cache, or load it from disk if cache miss.
    pub fn get_db(&self, mission_code: &str) -> Result<Arc<XtceDb>, XtceError> {
        // Read lock lookup
        {
            let cache = self.cache.read().unwrap();
            if let Some(db) = cache.get(mission_code) {
                return Ok(db.clone());
            }
        }

        // Cache miss: obtain write lock and compile
        let mut cache = self.cache.write().unwrap();
        // Check again in case another thread compiled it
        if let Some(db) = cache.get(mission_code) {
            return Ok(db.clone());
        }

        let db = Arc::new(self.load_from_disk(mission_code)?);
        cache.insert(mission_code.to_string(), db.clone());
        Ok(db)
    }

    fn load_from_disk(&self, mission_code: &str) -> Result<XtceDb, XtceError> {
        let filepath = Path::new(&self.db_dir).join(format!("{}.xml", mission_code));
        if !filepath.exists() {
            return Err(XtceError::ConfigError(format!(
                "XTCE file not found for mission '{mission_code}': {:?}",
                filepath
            )));
        }

        let content = std::fs::read_to_string(&filepath)
            .map_err(|e| XtceError::XmlParseError(format!("Failed to read file: {e}")))?;

        Self::parse_xtce(mission_code, &content)
    }

    /// Primary compiler: parses raw XML XTCE content into structured, indexed models.
    pub fn parse_xtce(mission_code: &str, content: &str) -> Result<XtceDb, XtceError> {
        let doc = roxmltree::Document::parse(content)
            .map_err(|e| XtceError::XmlParseError(format!("XML parser error: {e}")))?;

        let mut raw_params = HashMap::new(); // param_name -> type_ref
        let mut types = HashMap::new(); // type_name -> (ParameterType, Option<CalibratorType>, usize)

        let root = doc.root_element();
        if root.tag_name().name() != "SpaceSystem" {
            return Err(XtceError::XmlValidationError(
                "Root element must be SpaceSystem".to_string(),
            ));
        }

        // 1. Parse ParameterTypeSet
        for node in root.descendants() {
            let tag = node.tag_name().name();
            match tag {
                "IntegerParameterType" => {
                    let name = node.attribute("name").ok_or_else(|| {
                        XtceError::XmlValidationError("IntegerParameterType missing name".to_string())
                    })?;
                    let signed = node.attribute("signed").map(|s| s == "true").unwrap_or(false);
                    let mut size = 32;
                    
                    if let Some(encoding) = node.descendants().find(|n| {
                        let tag = n.tag_name().name();
                        tag == "IntegerDataEncoding" || tag == "DataEncoding"
                    }) {
                        if let Some(size_attr) = encoding.attribute("sizeInBits") {
                            size = size_attr.parse::<usize>().unwrap_or(32);
                        }
                    }

                    let calibrator = Self::parse_calibrator(&node);
                    let ptype = if signed { ParameterType::Int } else { ParameterType::Uint };
                    types.insert(name.to_string(), (ptype, calibrator, size));
                }
                "FloatParameterType" => {
                    let name = node.attribute("name").ok_or_else(|| {
                        XtceError::XmlValidationError("FloatParameterType missing name".to_string())
                    })?;
                    let mut size = 64;
                    if let Some(encoding) = node.descendants().find(|n| {
                        let tag = n.tag_name().name();
                        tag == "FloatDataEncoding" || tag == "DataEncoding"
                    }) {
                        if let Some(size_attr) = encoding.attribute("sizeInBits") {
                            size = size_attr.parse::<usize>().unwrap_or(64);
                        }
                    }
                    let calibrator = Self::parse_calibrator(&node);
                    types.insert(name.to_string(), (ParameterType::Float, calibrator, size));
                }
                "StringParameterType" => {
                    let name = node.attribute("name").ok_or_else(|| {
                        XtceError::XmlValidationError("StringParameterType missing name".to_string())
                    })?;
                    let mut size = 0;
                    if let Some(encoding) = node.descendants().find(|n| n.tag_name().name() == "StringDataEncoding") {
                        if let Some(size_attr) = encoding.attribute("sizeInBits") {
                            size = size_attr.parse::<usize>().unwrap_or(0);
                        }
                    }
                    types.insert(name.to_string(), (ParameterType::String, None, size));
                }
                "BooleanParameterType" => {
                    let name = node.attribute("name").ok_or_else(|| {
                        XtceError::XmlValidationError("BooleanParameterType missing name".to_string())
                    })?;
                    types.insert(name.to_string(), (ParameterType::Boolean, None, 1));
                }
                "BinaryParameterType" => {
                    let name = node.attribute("name").ok_or_else(|| {
                        XtceError::XmlValidationError("BinaryParameterType missing name".to_string())
                    })?;
                    let mut size = 0;
                    if let Some(encoding) = node.descendants().find(|n| n.tag_name().name() == "BinaryDataEncoding") {
                        if let Some(size_attr) = encoding.attribute("sizeInBits") {
                            size = size_attr.parse::<usize>().unwrap_or(0);
                        }
                    }
                    types.insert(name.to_string(), (ParameterType::Binary, None, size));
                }
                _ => {}
            }
        }

        // 2. Map Parameters to Type References
        for node in root.descendants() {
            if node.tag_name().name() == "Parameter" {
                if let (Some(name), Some(type_ref)) = (node.attribute("name"), node.attribute("parameterTypeRef")) {
                    raw_params.insert(name.to_string(), type_ref.to_string());
                }
            }
        }

        // 3. Construct Parameters Map
        let mut parameters = HashMap::new();
        let mut parameter_sizes = HashMap::new();

        for (pname, type_ref) in raw_params {
            if let Some((ptype, calibrator, size)) = types.get(&type_ref) {
                parameters.insert(
                    pname.clone(),
                    Parameter {
                        name: pname.clone(),
                        param_type: ptype.clone(),
                        calibrator: calibrator.clone(),
                    },
                );
                parameter_sizes.insert(pname, *size);
            }
        }

        // 4. Parse Containers & Calculate Absolute Bit Offsets
        let mut containers = HashMap::new();
        for node in root.descendants() {
            if node.tag_name().name() == "SequenceContainer" {
                let name = node.attribute("name").ok_or_else(|| {
                    XtceError::XmlValidationError("SequenceContainer missing name".to_string())
                })?;
                let inherits_from = node.attribute("inheritsFrom").map(|s| s.to_string());
                
                // Fetch the restriction criteria or APID matching logic
                let mut apid = node.attribute("apid").and_then(|s| s.parse::<u32>().ok());
                if apid.is_none() {
                    if let Some(base_cont) = node.children().find(|n| {
                        let tag = n.tag_name().name();
                        tag == "RestrictionCriteria" || tag == "BaseContainer"
                    }) {
                        let comparison = base_cont.descendants().find(|n| n.tag_name().name() == "Comparison");
                        apid = comparison.and_then(|c| {
                            let param_ref = c.attribute("parameterRef").unwrap_or("");
                            if param_ref.to_lowercase().contains("apid") {
                                c.attribute("value").and_then(|v| {
                                    if v.starts_with("0x") {
                                        u32::from_str_radix(&v[2..], 16).ok()
                                    } else {
                                        v.parse::<u32>().ok()
                                    }
                                })
                            } else {
                                None
                            }
                        });
                    }
                }

                let mut entries = Vec::new();
                let mut running_offset = 0;

                if let Some(entry_list) = node.children().find(|n| n.tag_name().name() == "EntryList") {
                    for entry_node in entry_list.children() {
                        if entry_node.tag_name().name() == "ParameterRefEntry" {
                            let param_ref = entry_node.attribute("parameterRef").ok_or_else(|| {
                                XtceError::XmlValidationError("ParameterRefEntry missing parameterRef".to_string())
                            })?;

                            if let Some(offset_str) = entry_node.attribute("startOffset") {
                                running_offset = offset_str.parse::<usize>().unwrap_or(running_offset);
                            }

                            let length_bits = *parameter_sizes.get(param_ref).unwrap_or(&0);
                            
                            entries.push(Entry {
                                parameter_name: param_ref.to_string(),
                                start_offset_bits: running_offset,
                                length_bits,
                            });

                            running_offset += length_bits;
                        }
                    }
                }

                let container = SequenceContainer {
                    name: name.to_string(),
                    inherits_from,
                    apid,
                    entries,
                };

                if let Some(apid_val) = apid {
                    containers.insert(apid_val, container);
                }
            }
        }

        Ok(XtceDb {
            mission_code: mission_code.to_string(),
            containers,
            parameters,
        })
    }

    fn parse_calibrator(node: &roxmltree::Node) -> Option<CalibratorType> {
        let default_cal = node.descendants().find(|n| n.tag_name().name() == "DefaultCalibrator")?;
        
        // Polynomial Calibrator
        if let Some(poly_node) = default_cal.descendants().find(|n| n.tag_name().name() == "PolynomialCalibrator") {
            let mut terms = Vec::new();
            for term in poly_node.descendants().filter(|n| n.tag_name().name() == "Term") {
                if let (Some(coeff_str), Some(exp_str)) = (term.attribute("coefficient"), term.attribute("exponent")) {
                    if let (Ok(coeff), Ok(exp)) = (coeff_str.parse::<f64>(), exp_str.parse::<usize>()) {
                        if exp >= terms.len() {
                            terms.resize(exp + 1, 0.0);
                        }
                        terms[exp] = coeff;
                    }
                }
            }
            if !terms.is_empty() {
                return Some(CalibratorType::Polynomial(PolynomialCalibrator { coefficients: terms }));
            }
        }

        // Spline Calibrator
        if let Some(spline_node) = default_cal.descendants().find(|n| {
            let tag = n.tag_name().name();
            tag == "SplineCalibrator" || tag == "Spline"
        }) {
            let mut points = Vec::new();
            for pt in spline_node.descendants().filter(|n| {
                let tag = n.tag_name().name();
                tag == "SplinePoint" || tag == "Point"
            }) {
                if let (Some(raw_str), Some(cal_str)) = (pt.attribute("raw"), pt.attribute("calibrated")) {
                    if let (Ok(raw), Ok(calibrated)) = (raw_str.parse::<f64>(), cal_str.parse::<f64>()) {
                        points.push(SplinePoint { raw, calibrated });
                    }
                }
            }
            points.sort_by(|a, b| a.raw.partial_cmp(&b.raw).unwrap_or(std::cmp::Ordering::Equal));
            if !points.is_empty() {
                return Some(CalibratorType::Spline(SplineCalibrator { points }));
            }
        }

        // State/Enum Calibrator
        if let Some(state_node) = default_cal.descendants().find(|n| {
            let tag = n.tag_name().name();
            tag == "StateCalibrator" || tag == "StateMap"
        }) {
            let mut mappings = HashMap::new();
            for st in state_node.descendants().filter(|n| {
                let tag = n.tag_name().name();
                tag == "State" || tag == "ValueMap"
            }) {
                if let (Some(val_str), Some(label)) = (st.attribute("value"), st.attribute("label")) {
                    if let Ok(val) = val_str.parse::<i64>() {
                        mappings.insert(val, label.to_string());
                    }
                }
            }
            if !mappings.is_empty() {
                return Some(CalibratorType::State(StateCalibrator { state_mappings: mappings }));
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_XTCE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<SpaceSystem xmlns="http://www.omg.org/space/xtce" name="TestMission">
  <TelemetryMetadata>
    <ParameterSet>
      <Parameter name="Volt" parameterTypeRef="VoltType"/>
      <Parameter name="Temp" parameterTypeRef="TempType"/>
      <Parameter name="Status" parameterTypeRef="StatusType"/>

      <ParameterTypeSet>
        <IntegerParameterType name="VoltType" signed="false">
          <IntegerDataEncoding sizeInBits="12"/>
          <DefaultCalibrator>
            <PolynomialCalibrator>
              <Term coefficient="0.02" exponent="1"/>
            </PolynomialCalibrator>
          </DefaultCalibrator>
        </IntegerParameterType>

        <IntegerParameterType name="TempType" signed="true">
          <IntegerDataEncoding sizeInBits="8"/>
        </IntegerParameterType>

        <IntegerParameterType name="StatusType" signed="false">
          <IntegerDataEncoding sizeInBits="4"/>
          <DefaultCalibrator>
            <StateCalibrator>
              <State value="0" label="OFF"/>
              <State value="1" label="ON"/>
              <State value="10" label="TRICKLE"/>
            </StateCalibrator>
          </DefaultCalibrator>
        </IntegerParameterType>
      </ParameterTypeSet>
    </ParameterSet>

    <ContainerSet>
      <SequenceContainer name="MainContainer" apid="42">
        <EntryList>
          <ParameterRefEntry parameterRef="Volt"/>
          <ParameterRefEntry parameterRef="Temp"/>
          <ParameterRefEntry parameterRef="Status"/>
        </EntryList>
      </SequenceContainer>
    </ContainerSet>
  </TelemetryMetadata>
</SpaceSystem>
"#;

    #[test]
    fn test_parse_xtce() {
        let db = XtceRegistry::parse_xtce("test", TEST_XTCE).unwrap();
        assert_eq!(db.mission_code, "test");
        
        // Check parameters
        assert!(db.parameters.contains_key("Volt"));
        assert!(db.parameters.contains_key("Temp"));
        assert!(db.parameters.contains_key("Status"));

        let volt = db.parameters.get("Volt").unwrap();
        assert_eq!(volt.param_type, ParameterType::Uint);
        
        if let Some(CalibratorType::Polynomial(poly)) = &volt.calibrator {
            assert_eq!(poly.coefficients[1], 0.02);
        } else {
            panic!("Expected polynomial calibrator");
        }

        let status = db.parameters.get("Status").unwrap();
        if let Some(CalibratorType::State(state)) = &status.calibrator {
            assert_eq!(state.state_mappings.get(&10).unwrap(), "TRICKLE");
        } else {
            panic!("Expected state calibrator");
        }

        // Check containers
        let container = db.containers.get(&42).unwrap();
        assert_eq!(container.name, "MainContainer");
        assert_eq!(container.apid, Some(42));
        assert_eq!(container.entries.len(), 3);

        // Check offsets
        assert_eq!(container.entries[0].parameter_name, "Volt");
        assert_eq!(container.entries[0].start_offset_bits, 0);
        assert_eq!(container.entries[0].length_bits, 12);

        assert_eq!(container.entries[1].parameter_name, "Temp");
        assert_eq!(container.entries[1].start_offset_bits, 12);
        assert_eq!(container.entries[1].length_bits, 8);

        assert_eq!(container.entries[2].parameter_name, "Status");
        assert_eq!(container.entries[2].start_offset_bits, 20);
        assert_eq!(container.entries[2].length_bits, 4);
    }
}
