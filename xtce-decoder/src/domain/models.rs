use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ParameterType {
    Uint,
    Int,
    Float,
    String,
    Boolean,
    Binary,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CalibratorType {
    Polynomial(PolynomialCalibrator),
    Spline(SplineCalibrator),
    State(StateCalibrator),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolynomialCalibrator {
    pub coefficients: Vec<f64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SplinePoint {
    pub raw: f64,
    pub calibrated: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SplineCalibrator {
    pub points: Vec<SplinePoint>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StateCalibrator {
    pub state_mappings: HashMap<i64, String>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub parameter_name: String,
    pub start_offset_bits: usize,
    pub length_bits: usize,
}

#[derive(Debug, Clone)]
pub struct SequenceContainer {
    pub name: String,
    pub inherits_from: Option<String>,
    pub apid: Option<u32>,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub param_type: ParameterType,
    pub calibrator: Option<CalibratorType>,
}

#[derive(Debug, Clone)]
pub struct XtceDb {
    pub mission_code: String,
    pub containers: HashMap<u32, SequenceContainer>, // APID -> Container
    pub parameters: HashMap<String, Parameter>,
}
