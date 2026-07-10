use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use crate::domain::errors::DomainError;
use crate::domain::models::{DerivedDb, DerivedParameterDefinition};

pub struct FormulaRegistry {
    config_dir: PathBuf,
    cache: RwLock<HashMap<String, Arc<DerivedDb>>>,
}

impl FormulaRegistry {
    pub fn new<P: AsRef<Path>>(config_dir: P) -> Self {
        Self {
            config_dir: config_dir.as_ref().to_path_buf(),
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Retrieve the parsed, sorted, and cached derived parameter configuration for a mission.
    pub fn get_db(&self, mission_code: &str) -> Result<Arc<DerivedDb>, DomainError> {
        // Read lock check
        {
            let cache = self.cache.read().unwrap();
            if let Some(db) = cache.get(mission_code) {
                return Ok(Arc::clone(db));
            }
        }

        // Cache miss: write lock
        let mut cache = self.cache.write().unwrap();
        // Check again in case another thread loaded it concurrently
        if let Some(db) = cache.get(mission_code) {
            return Ok(Arc::clone(db));
        }

        match self.load_db_from_file(mission_code) {
            Ok(db) => {
                let arc_db = Arc::new(db);
                cache.insert(mission_code.to_string(), Arc::clone(&arc_db));
                metrics::gauge!("ecs_db_cache_size").set(cache.len() as f64);
                Ok(arc_db)
            }
            Err(e) => {
                metrics::counter!("ecs_db_load_errors_total", "mission" => mission_code.to_string()).increment(1);
                Err(e)
            }
        }
    }

    /// Load and validate a configuration YAML file from the filesystem.
    fn load_db_from_file(&self, mission_code: &str) -> Result<DerivedDb, DomainError> {
        if !self.config_dir.exists() {
            return Err(DomainError::ConfigDirNotFound(self.config_dir.to_string_lossy().to_string()));
        }

        let filepath = self.config_dir.join(format!("{}.yaml", mission_code));
        if !filepath.exists() {
            return Err(DomainError::ConfigFileNotFound(
                mission_code.to_string(),
                filepath.to_string_lossy().to_string(),
            ));
        }

        let content = fs::read_to_string(&filepath).map_err(|e| {
            DomainError::ConfigReadError(mission_code.to_string(), e.to_string())
        })?;

        #[derive(serde::Deserialize)]
        struct RawConfig {
            derived_parameters: Vec<DerivedParameterDefinition>,
        }

        let raw_config: RawConfig = serde_yaml::from_str(&content).map_err(|e| {
            DomainError::ConfigParseError(mission_code.to_string(), e.to_string())
        })?;

        // 1. Verify no duplicate derived parameter names
        let mut names = HashSet::new();
        for dp in &raw_config.derived_parameters {
            if !names.insert(dp.name.clone()) {
                return Err(DomainError::DuplicateParameter(dp.name.clone()));
            }
        }

        // 2. Validate expression syntactical compilation
        for dp in &raw_config.derived_parameters {
            if let Err(e) = evalexpr::build_operator_tree(&dp.expression) {
                return Err(DomainError::InvalidExpression(dp.name.clone(), e.to_string()));
            }
        }

        // 3. Perform topological sort & cyclic check (Kahn's Algorithm)
        let sorted_params = sort_topologically(mission_code, raw_config.derived_parameters)?;

        Ok(DerivedDb {
            mission_code: mission_code.to_string(),
            derived_parameters: sorted_params,
        })
    }

    /// Clears the cache (useful for testing or config hot-reload).
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.clear();
        metrics::gauge!("ecs_db_cache_size").set(0.0);
    }
}

/// Sort derived parameters topologically using Kahn's algorithm to resolve evaluation order.
fn sort_topologically(
    mission_code: &str,
    params: Vec<DerivedParameterDefinition>,
) -> Result<Vec<DerivedParameterDefinition>, DomainError> {
    let mut name_to_def: HashMap<String, DerivedParameterDefinition> = params
        .into_iter()
        .map(|d| (d.name.clone(), d))
        .collect();

    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut adjacency_list: HashMap<String, Vec<String>> = HashMap::new();

    // Initialize in_degree table
    for name in name_to_def.keys() {
        in_degree.insert(name.clone(), 0);
    }

    // Build the dependency graph
    for (name, def) in &name_to_def {
        let mut dep_count = 0;
        for input in &def.inputs {
            // An edge exists if one derived parameter depends on another derived parameter
            if name_to_def.contains_key(&input.parameter_name) {
                dep_count += 1;
                adjacency_list
                    .entry(input.parameter_name.clone())
                    .or_default()
                    .push(name.clone());
            }
        }
        in_degree.insert(name.clone(), dep_count);
    }

    // Initialize queue with parameters having 0 in-degree dependencies
    let mut queue: VecDeque<String> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(name, _)| name.clone())
        .collect();

    let mut sorted_names = Vec::new();

    while let Some(name) = queue.pop_front() {
        sorted_names.push(name.clone());
        if let Some(deps) = adjacency_list.get(&name) {
            for dep in deps {
                if let Some(deg) = in_degree.get_mut(dep) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }
    }

    // If we could not sort all elements, a cycle exists
    if sorted_names.len() < name_to_def.len() {
        let cyclic_params: Vec<String> = name_to_def
            .keys()
            .filter(|name| !sorted_names.contains(name))
            .cloned()
            .collect();
        return Err(DomainError::CyclicDependency(
            mission_code.to_string(),
            cyclic_params.join(", "),
        ));
    }

    // Constructs final sorted vector
    let sorted_defs = sorted_names
        .into_iter()
        .map(|name| name_to_def.remove(&name).unwrap())
        .collect();

    Ok(sorted_defs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_dir(dir_name: &str) -> PathBuf {
        let path = PathBuf::from(dir_name);
        if path.exists() {
            let _ = fs::remove_dir_all(&path);
        }
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn cleanup_test_dir(path: PathBuf) {
        if path.exists() {
            let _ = fs::remove_dir_all(path);
        }
    }

    #[test]
    fn test_registry_happy_path() {
        let temp_dir = setup_test_dir("./temp_test_registry_happy");
        let mission = "m1";
        let yaml_content = r#"
derived_parameters:
  - name: "/SC/EPS/BatteryPower"
    inputs:
      - parameter_name: "/SC/EPS/BatteryVoltage"
        alias: "v"
      - parameter_name: "/SC/EPS/BatteryCurrent"
        alias: "i"
    expression: "v * i"
    unit: "W"
  - name: "/SC/Thermal/DiffTemp"
    inputs:
      - parameter_name: "/SC/Thermal/Temp1"
        alias: "t1"
      - parameter_name: "/SC/Thermal/Temp2"
        alias: "t2"
    expression: "t1 - t2"
    unit: "degC"
"#;

        fs::write(temp_dir.join(format!("{}.yaml", mission)), yaml_content).unwrap();

        let registry = FormulaRegistry::new(&temp_dir);
        
        // 1. First load (cache miss)
        let db = registry.get_db(mission).unwrap();
        assert_eq!(db.mission_code, mission);
        assert_eq!(db.derived_parameters.len(), 2);
        let names: HashSet<String> = db.derived_parameters.iter().map(|p| p.name.clone()).collect();
        assert!(names.contains("/SC/EPS/BatteryPower"));
        assert!(names.contains("/SC/Thermal/DiffTemp"));

        // 2. Second load (cache hit)
        let db2 = registry.get_db(mission).unwrap();
        assert!(Arc::ptr_eq(&db, &db2));

        // 3. Clear cache
        registry.clear_cache();
        let db3 = registry.get_db(mission).unwrap();
        assert!(!Arc::ptr_eq(&db, &db3));

        cleanup_test_dir(temp_dir);
    }

    #[test]
    fn test_registry_topological_sort() {
        let temp_dir = setup_test_dir("./temp_test_registry_topo");
        let mission = "m2";
        // Total depends on Sub1, but Total is defined first in YAML
        let yaml_content = r#"
derived_parameters:
  - name: "/SC/Power/Total"
    inputs:
      - parameter_name: "/SC/Power/Sub1"
        alias: "s1"
      - parameter_name: "/SC/Power/Solar"
        alias: "s2"
    expression: "s1 + s2"
  - name: "/SC/Power/Sub1"
    inputs:
      - parameter_name: "/SC/EPS/BatteryPower"
        alias: "bp"
    expression: "bp * 0.9"
"#;

        fs::write(temp_dir.join(format!("{}.yaml", mission)), yaml_content).unwrap();

        let registry = FormulaRegistry::new(&temp_dir);
        let db = registry.get_db(mission).unwrap();

        assert_eq!(db.derived_parameters.len(), 2);
        // Sub1 must be evaluated before Total
        assert_eq!(db.derived_parameters[0].name, "/SC/Power/Sub1");
        assert_eq!(db.derived_parameters[1].name, "/SC/Power/Total");

        cleanup_test_dir(temp_dir);
    }

    #[test]
    fn test_registry_cyclic_dependency() {
        let temp_dir = setup_test_dir("./temp_test_registry_cycle");
        let mission = "m3";
        // A depends on B, B depends on A
        let yaml_content = r#"
derived_parameters:
  - name: "/SC/A"
    inputs:
      - parameter_name: "/SC/B"
        alias: "b"
    expression: "b * 2"
  - name: "/SC/B"
    inputs:
      - parameter_name: "/SC/A"
        alias: "a"
    expression: "a + 1"
"#;

        fs::write(temp_dir.join(format!("{}.yaml", mission)), yaml_content).unwrap();

        let registry = FormulaRegistry::new(&temp_dir);
        let err = registry.get_db(mission).unwrap_err();
        assert!(matches!(err, DomainError::CyclicDependency(..)));

        cleanup_test_dir(temp_dir);
    }

    #[test]
    fn test_registry_duplicate_parameter() {
        let temp_dir = setup_test_dir("./temp_test_registry_dup");
        let mission = "m4";
        let yaml_content = r#"
derived_parameters:
  - name: "/SC/A"
    inputs: []
    expression: "5.0"
  - name: "/SC/A"
    inputs: []
    expression: "10.0"
"#;

        fs::write(temp_dir.join(format!("{}.yaml", mission)), yaml_content).unwrap();

        let registry = FormulaRegistry::new(&temp_dir);
        let err = registry.get_db(mission).unwrap_err();
        assert!(matches!(err, DomainError::DuplicateParameter(..)));

        cleanup_test_dir(temp_dir);
    }

    #[test]
    fn test_registry_invalid_expression() {
        let temp_dir = setup_test_dir("./temp_test_registry_invalid_expr");
        let mission = "m5";
        let yaml_content = r#"
derived_parameters:
  - name: "/SC/A"
    inputs: []
    expression: "5.0 * ("
"#;

        fs::write(temp_dir.join(format!("{}.yaml", mission)), yaml_content).unwrap();

        let registry = FormulaRegistry::new(&temp_dir);
        let err = registry.get_db(mission).unwrap_err();
        assert!(matches!(err, DomainError::InvalidExpression(..)));

        cleanup_test_dir(temp_dir);
    }
}
