use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct InputMapping {
    pub parameter_name: String,
    pub alias: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct DerivedParameterDefinition {
    pub name: String,
    pub inputs: Vec<InputMapping>,
    pub expression: String,
    pub unit: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct DerivedDb {
    pub mission_code: String,
    pub derived_parameters: Vec<DerivedParameterDefinition>,
}
