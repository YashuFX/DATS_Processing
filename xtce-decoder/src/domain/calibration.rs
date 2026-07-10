use crate::domain::errors::XtceError;
use crate::domain::models::{CalibratorType, Parameter, PolynomialCalibrator, SplineCalibrator, StateCalibrator};
use crate::domain::decommutation::DecommutatedParameter;
use crate::proto::{TelemetryParameter, ParameterValue, parameter_value::Value, ParameterValidity};

impl PolynomialCalibrator {
    pub fn calibrate(&self, raw: f64) -> f64 {
        let mut result = 0.0;
        let mut x_pow = 1.0;
        for coeff in &self.coefficients {
            result += coeff * x_pow;
            x_pow *= raw;
        }
        result
    }
}

impl SplineCalibrator {
    pub fn calibrate(&self, raw: f64) -> Result<f64, XtceError> {
        if self.points.is_empty() {
            return Err(XtceError::CalibrationFailed("Spline calibrator contains no points".to_string()));
        }
        if self.points.len() == 1 {
            return Ok(self.points[0].calibrated);
        }
        
        let first = &self.points[0];
        if raw <= first.raw {
            let second = &self.points[1];
            let dx = second.raw - first.raw;
            if dx.abs() < 1e-9 {
                return Ok(first.calibrated);
            }
            let slope = (second.calibrated - first.calibrated) / dx;
            return Ok(first.calibrated + slope * (raw - first.raw));
        }

        let last = &self.points[self.points.len() - 1];
        if raw >= last.raw {
            let prev_last = &self.points[self.points.len() - 2];
            let dx = last.raw - prev_last.raw;
            if dx.abs() < 1e-9 {
                return Ok(last.calibrated);
            }
            let slope = (last.calibrated - prev_last.calibrated) / dx;
            return Ok(last.calibrated + slope * (raw - last.raw));
        }

        for i in 0..self.points.len() - 1 {
            let p1 = &self.points[i];
            let p2 = &self.points[i + 1];
            if raw >= p1.raw && raw <= p2.raw {
                let dx = p2.raw - p1.raw;
                if dx.abs() < 1e-9 {
                    return Ok(p1.calibrated);
                }
                let slope = (p2.calibrated - p1.calibrated) / dx;
                return Ok(p1.calibrated + slope * (raw - p1.raw));
            }
        }

        Ok(first.calibrated)
    }
}

impl StateCalibrator {
    pub fn calibrate(&self, raw: i64) -> Result<String, XtceError> {
        if let Some(state) = self.state_mappings.get(&raw) {
            Ok(state.clone())
        } else {
            Err(XtceError::CalibrationFailed(format!(
                "State calibrator has no mapping for raw value {}",
                raw
            )))
        }
    }
}

pub struct CalibrationEngine;

impl CalibrationEngine {
    /// Applies the appropriate calibrator to the decommutated parameter value.
    pub fn calibrate(
        decom_param: &DecommutatedParameter,
        param_def: &Parameter,
    ) -> Result<TelemetryParameter, XtceError> {
        let name = decom_param.name.clone();
        let raw_val = decom_param.raw_value.clone();
        
        let (eng_val, validity) = match &param_def.calibrator {
            None => {
                // No calibration required
                (raw_val.clone(), ParameterValidity::Valid as i32)
            }
            Some(CalibratorType::Polynomial(poly)) => {
                match get_raw_as_f64(&raw_val) {
                    Some(raw_f) => {
                        let calibrated = poly.calibrate(raw_f);
                        let eng = ParameterValue {
                            value: Some(Value::FloatValue(calibrated)),
                        };
                        (eng, ParameterValidity::Valid as i32)
                    }
                    None => {
                        (raw_val.clone(), ParameterValidity::Invalid as i32)
                    }
                }
            }
            Some(CalibratorType::Spline(spline)) => {
                match get_raw_as_f64(&raw_val) {
                    Some(raw_f) => {
                        match spline.calibrate(raw_f) {
                            Ok(calibrated) => {
                                let eng = ParameterValue {
                                    value: Some(Value::FloatValue(calibrated)),
                                };
                                (eng, ParameterValidity::Valid as i32)
                            }
                            Err(_) => {
                                (raw_val.clone(), ParameterValidity::Invalid as i32)
                            }
                        }
                    }
                    None => {
                        (raw_val.clone(), ParameterValidity::Invalid as i32)
                    }
                }
            }
            Some(CalibratorType::State(state)) => {
                match get_raw_as_i64(&raw_val) {
                    Some(raw_i) => {
                        match state.calibrate(raw_i) {
                            Ok(label) => {
                                let eng = ParameterValue {
                                    value: Some(Value::StringValue(label)),
                                };
                                (eng, ParameterValidity::Valid as i32)
                            }
                            Err(_) => {
                                (raw_val.clone(), ParameterValidity::Invalid as i32)
                            }
                        }
                    }
                    None => {
                        (raw_val.clone(), ParameterValidity::Invalid as i32)
                    }
                }
            }
        };

        Ok(TelemetryParameter {
            name,
            raw_value: Some(raw_val),
            engineering_value: Some(eng_val),
            validity,
        })
    }
}

fn get_raw_as_f64(val: &ParameterValue) -> Option<f64> {
    match &val.value {
        Some(Value::IntValue(i)) => Some(*i as f64),
        Some(Value::FloatValue(f)) => Some(*f),
        Some(Value::BoolValue(b)) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn get_raw_as_i64(val: &ParameterValue) -> Option<i64> {
    match &val.value {
        Some(Value::IntValue(i)) => Some(*i),
        Some(Value::FloatValue(f)) => Some(*f as i64),
        Some(Value::BoolValue(b)) => Some(if *b { 1 } else { 0 }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::SplinePoint;
    use std::collections::HashMap;

    #[test]
    fn test_polynomial_calibration() {
        // y = 1.0 + 2.0x + 0.5x^2
        let poly = PolynomialCalibrator {
            coefficients: vec![1.0, 2.0, 0.5],
        };
        let raw = 10.0;
        let expected = 1.0 + 2.0 * 10.0 + 0.5 * 10.0 * 10.0; // 1.0 + 20.0 + 50.0 = 71.0
        assert_eq!(poly.calibrate(raw), expected);
    }

    #[test]
    fn test_spline_calibration() {
        let spline = SplineCalibrator {
            points: vec![
                SplinePoint { raw: 0.0, calibrated: 0.0 },
                SplinePoint { raw: 10.0, calibrated: 100.0 },
                SplinePoint { raw: 20.0, calibrated: 300.0 },
            ],
        };

        // Interpolation
        assert_eq!(spline.calibrate(5.0).unwrap(), 50.0);
        assert_eq!(spline.calibrate(15.0).unwrap(), 200.0);

        // Extrapolations
        assert_eq!(spline.calibrate(-5.0).unwrap(), -50.0);
        assert_eq!(spline.calibrate(25.0).unwrap(), 400.0);
    }

    #[test]
    fn test_state_calibration() {
        let mut map = HashMap::new();
        map.insert(0, "OFF".to_string());
        map.insert(1, "ON".to_string());
        let state = StateCalibrator { state_mappings: map };

        assert_eq!(state.calibrate(0).unwrap(), "OFF");
        assert_eq!(state.calibrate(1).unwrap(), "ON");
        assert!(state.calibrate(2).is_err());
    }
}
