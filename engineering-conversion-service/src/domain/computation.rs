use crate::domain::errors::DomainError;
use crate::domain::models::DerivedParameterDefinition;
use crate::proto::{
    parameter_value::Value as ProtoValue, ParameterValue, ParameterValidity, TelemetryParameter,
};
use evalexpr::{ContextWithMutableVariables, Value as EvalValue};

pub struct ComputationEngine;

impl ComputationEngine {
    /// Evaluates a derived parameter using the input variables and values from the envelope.
    pub fn evaluate(
        definition: &DerivedParameterDefinition,
        available_params: &[TelemetryParameter],
    ) -> Result<TelemetryParameter, DomainError> {
        let mut context = evalexpr::HashMapContext::new();

        for input in &definition.inputs {
            // Find parameter in available parameters list
            let param = available_params
                .iter()
                .find(|p| p.name == input.parameter_name)
                .ok_or_else(|| {
                    DomainError::MissingInputParameter(
                        input.parameter_name.clone(),
                        definition.name.clone(),
                    )
                })?;

            // Retrieve engineering value (fallback to raw if missing)
            let proto_val = param
                .engineering_value
                .as_ref()
                .or(param.raw_value.as_ref())
                .ok_or_else(|| {
                    DomainError::MissingInputParameter(
                        input.parameter_name.clone(),
                        definition.name.clone(),
                    )
                })?;

            let eval_val = proto_value_to_eval_value(proto_val).ok_or_else(|| {
                DomainError::TypeConversionError(format!(
                    "Unsupported type for input parameter '{}'",
                    input.parameter_name
                ))
            })?;

            context
                .set_value(input.alias.clone().into(), eval_val)
                .map_err(|e| {
                    DomainError::EvaluationError(definition.name.clone(), e.to_string())
                })?;
        }

        // Evaluate mathematical / logical expression
        let result_val = evalexpr::eval_with_context(&definition.expression, &context)
            .map_err(|e| {
                DomainError::EvaluationError(definition.name.clone(), e.to_string())
            })?;

        // Convert the result back to a protobuf parameter value
        let engineering_value = eval_value_to_proto_value(result_val).ok_or_else(|| {
            DomainError::TypeConversionError(format!(
                "Failed to convert evaluated result of '{}' to protobuf type",
                definition.name
            ))
        })?;

        Ok(TelemetryParameter {
            name: definition.name.clone(),
            raw_value: None,
            engineering_value: Some(engineering_value),
            validity: ParameterValidity::Valid as i32,
        })
    }
}

fn proto_value_to_eval_value(pv: &ParameterValue) -> Option<EvalValue> {
    let val_union = pv.value.as_ref()?;
    match val_union {
        ProtoValue::IntValue(i) => Some(EvalValue::Int(*i)),
        ProtoValue::FloatValue(f) => Some(EvalValue::Float(*f)),
        ProtoValue::BoolValue(b) => Some(EvalValue::Boolean(*b)),
        ProtoValue::StringValue(s) => Some(EvalValue::String(s.clone())),
        ProtoValue::BytesValue(_) => None, // Bytes are unsupported in expression logic
    }
}

fn eval_value_to_proto_value(ev: EvalValue) -> Option<ParameterValue> {
    let value = match ev {
        EvalValue::Int(i) => Some(ProtoValue::IntValue(i)),
        EvalValue::Float(f) => Some(ProtoValue::FloatValue(f)),
        EvalValue::Boolean(b) => Some(ProtoValue::BoolValue(b)),
        EvalValue::String(s) => Some(ProtoValue::StringValue(s)),
        _ => None,
    };
    value.map(|v| ParameterValue { value: Some(v) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::InputMapping;

    fn make_param(name: &str, value: ProtoValue) -> TelemetryParameter {
        TelemetryParameter {
            name: name.to_string(),
            raw_value: None,
            engineering_value: Some(ParameterValue { value: Some(value) }),
            validity: ParameterValidity::Valid as i32,
        }
    }

    #[test]
    fn test_evaluate_happy_path_math() {
        let definition = DerivedParameterDefinition {
            name: "/SC/EPS/BatteryPower".to_string(),
            inputs: vec![
                InputMapping {
                    parameter_name: "/SC/EPS/BatteryVoltage".to_string(),
                    alias: "v".to_string(),
                },
                InputMapping {
                    parameter_name: "/SC/EPS/BatteryCurrent".to_string(),
                    alias: "i".to_string(),
                },
            ],
            expression: "v * i".to_string(),
            unit: Some("W".to_string()),
        };

        let available_params = vec![
            make_param("/SC/EPS/BatteryVoltage", ProtoValue::FloatValue(28.2)),
            make_param("/SC/EPS/BatteryCurrent", ProtoValue::FloatValue(4.5)),
        ];

        let result = ComputationEngine::evaluate(&definition, &available_params).unwrap();
        assert_eq!(result.name, "/SC/EPS/BatteryPower");
        let val = result.engineering_value.unwrap().value.unwrap();
        if let ProtoValue::FloatValue(f) = val {
            assert!((f - 126.9).abs() < 1e-9);
        } else {
            panic!("Expected FloatValue");
        }
        assert_eq!(result.validity, ParameterValidity::Valid as i32);
    }

    #[test]
    fn test_evaluate_logical_ops() {
        let definition = DerivedParameterDefinition {
            name: "/SC/Payload/IsActive".to_string(),
            inputs: vec![
                InputMapping {
                    parameter_name: "/SC/Payload/Voltage".to_string(),
                    alias: "v".to_string(),
                },
                InputMapping {
                    parameter_name: "/SC/Payload/Current".to_string(),
                    alias: "i".to_string(),
                },
            ],
            expression: "(v > 20.0) && (i > 0.5)".to_string(),
            unit: None,
        };

        // Case 1: True
        let available_params = vec![
            make_param("/SC/Payload/Voltage", ProtoValue::FloatValue(24.0)),
            make_param("/SC/Payload/Current", ProtoValue::FloatValue(1.2)),
        ];
        let result = ComputationEngine::evaluate(&definition, &available_params).unwrap();
        assert_eq!(
            result.engineering_value.unwrap().value.unwrap(),
            ProtoValue::BoolValue(true)
        );

        // Case 2: False
        let available_params = vec![
            make_param("/SC/Payload/Voltage", ProtoValue::FloatValue(18.0)),
            make_param("/SC/Payload/Current", ProtoValue::FloatValue(1.2)),
        ];
        let result = ComputationEngine::evaluate(&definition, &available_params).unwrap();
        assert_eq!(
            result.engineering_value.unwrap().value.unwrap(),
            ProtoValue::BoolValue(false)
        );
    }

    #[test]
    fn test_evaluate_conditional() {
        let definition = DerivedParameterDefinition {
            name: "/SC/Thermal/HeaterState".to_string(),
            inputs: vec![
                InputMapping {
                    parameter_name: "/SC/Thermal/Temp".to_string(),
                    alias: "t".to_string(),
                },
            ],
            expression: "if(t < 15.0, 1, 0)".to_string(),
            unit: None,
        };

        // Temp = 10.0 -> State should be 1
        let available_params = vec![
            make_param("/SC/Thermal/Temp", ProtoValue::FloatValue(10.0)),
        ];
        let result = ComputationEngine::evaluate(&definition, &available_params).unwrap();
        assert_eq!(
            result.engineering_value.unwrap().value.unwrap(),
            ProtoValue::IntValue(1)
        );

        // Temp = 20.0 -> State should be 0
        let available_params = vec![
            make_param("/SC/Thermal/Temp", ProtoValue::FloatValue(20.0)),
        ];
        let result = ComputationEngine::evaluate(&definition, &available_params).unwrap();
        assert_eq!(
            result.engineering_value.unwrap().value.unwrap(),
            ProtoValue::IntValue(0)
        );
    }

    #[test]
    fn test_evaluate_missing_input() {
        let definition = DerivedParameterDefinition {
            name: "/SC/EPS/BatteryPower".to_string(),
            inputs: vec![
                InputMapping {
                    parameter_name: "/SC/EPS/BatteryVoltage".to_string(),
                    alias: "v".to_string(),
                },
            ],
            expression: "v * 2.0".to_string(),
            unit: None,
        };

        let available_params = vec![];
        let err = ComputationEngine::evaluate(&definition, &available_params).unwrap_err();
        assert!(matches!(err, DomainError::MissingInputParameter(..)));
    }
}
