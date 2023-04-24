use std::{
    collections::HashMap,
    fs::{self, DirEntry, File},
    io::Read,
    path::Path,
    result::Result::Ok,
    sync::Arc,
};

use anyhow::{anyhow, Result};

use super::ServiceOpts;

fn read_service_options<R: Read>(name: String, reader: R) -> Result<ServiceOpts> {
    let mut opts: ServiceOpts = serde_yaml::from_reader(reader)?;
    opts.name = name;
    Ok(opts)
}

pub fn try_read_service(name: String, dir_path: &Path) -> Result<ServiceOpts> {
    // ReadService tries to read a service with name `name` inside a directory located at `path`.
    // first it tries to find `path`/`name`.`yaml` then tries to find `path`/`name`.yml`.
    let path = dir_path.join(Path::new(&format!("{name}.yaml")));
    if let Ok(file) = File::open(path) {
        return read_service_options(name, file);
    }
    let path = dir_path.join(Path::new(&format!("{name}.yml")));
    if let Ok(file) = File::open(path) {
        return read_service_options(name, file);
    }

    Err(anyhow!(
        "service with name {} not found in {}",
        name,
        dir_path.display()
    ))
}

pub fn read_all(dir_path: &Path) -> Result<HashMap<String, Arc<ServiceOpts>>> {
    // this should read all service configuration files from dir_path, return a vector of service options
    let mut options = HashMap::new();
    let entries = fs::read_dir(dir_path)?;

    for entry_res in entries {
        let dir_entry = entry_res?;
        let file_type = dir_entry.file_type()?;
        if !file_type.is_file() {
            return Err(anyhow::anyhow!(
                "path {} contains non-regular file {}",
                dir_path.display(),
                dir_entry.path().display()
            ));
        }
        let file = File::open(dir_entry.path())?;
        let service_name = get_service_name(dir_entry)?;
        let opts = read_service_options(service_name, file)?;
        options.insert(opts.name.clone(), Arc::new(opts));
    }

    Ok(options)
}

fn get_service_name(entry: DirEntry) -> Result<String> {
    if let Some(file_name) = entry.file_name().to_str() {
        let split_name: Vec<&str> = file_name.split('.').collect();
        if split_name.len() != 2 {
            return Err(anyhow!(
                "file {} name is invalid. should be <name>.<yaml/yml>",
                file_name
            ));
        }
        if split_name[1] != "yaml" && split_name[1] != "yml" {
            return Err(anyhow!(
                "file {} name is invalid. file extension should be yaml or yml",
                file_name
            ));
        }
        return Ok(split_name[0].to_string());
    }

    Err(anyhow!(
        "file name at {} is not valid unicode",
        entry.path().display()
    ))
}
// if a user wants to add a service, they should provide the service name, loader looks for name.[yaml][yml] file in designated path, loads service options

#[cfg(test)]
mod test {
    use super::{read_all, read_service_options};
    use crate::manager::ServiceOpts;
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::{NamedTempFile, TempDir};

    #[test]
    fn test_read_service_options_valid() {
        let name = "s1";
        let mut tmpfile = NamedTempFile::new().unwrap();
        tmpfile.write_all(b"\ncmd: echo hi\nhealth_check: sleep 1\ncombine_log: false\none_shot: false\nafter: \n - s1\n - s2").unwrap();
        let file = tmpfile.reopen().unwrap();
        let opts = read_service_options(name.to_string(), file).unwrap();
        let given = ServiceOpts {
            name: "s1".to_string(),
            cmd: "echo hi".to_string(),
            health_check: "sleep 1".to_string(),
            combine_log: false,
            one_shot: false,
            after: Vec::from(["s1".to_string(), "s2".to_string()]),
        };
        assert_eq!(opts, given);
    }

    #[test]
    fn test_read_all_valid() {
        // directory has multiple valid service options yaml files
        let tmpdir = TempDir::new().unwrap();
        let mut rng = thread_rng();
        let mut given_options_map = HashMap::new();
        for _ in 0..rng.gen_range(1..10) {
            let opts = generate_random_service();
            let file_name = format!("{}.yaml", opts.name);
            let tmpfile = File::create(tmpdir.path().join(file_name)).unwrap();
            serde_yaml::to_writer(tmpfile, &opts).unwrap();
            given_options_map.insert(opts.name.clone(), Arc::new(opts));
        }

        let got_options_map = read_all(tmpdir.path()).unwrap();

        assert_eq!(given_options_map, got_options_map);
    }

    #[test]
    fn test_read_all_invalid_options() {
        // directory has invalid service options yaml files
        let tmpdir = TempDir::new().unwrap();
        let mut rng = thread_rng();
        for _ in 0..rng.gen_range(1..10) {
            let opts = generate_random_service();
            let file_name = format!("{}.yaml", opts.name);
            let tmpfile = File::create(tmpdir.path().join(file_name)).unwrap();
            serde_yaml::to_writer(tmpfile, &opts).unwrap();
        }
        let read_result = read_all(tmpdir.path());
        assert!(read_result.is_ok());

        let file_name = "s1.yaml".to_string();
        let tmpfile = File::create(tmpdir.path().join(file_name)).unwrap();
        serde_yaml::to_writer(tmpfile, "some invalid options text").unwrap();
        let read_result = read_all(tmpdir.path());
        assert!(read_result.is_err());
    }

    #[test]
    fn test_read_all_non_yaml() {
        // director has non yaml files
        let tmpdir = TempDir::new().unwrap();
        let mut rng = thread_rng();
        for _ in 0..rng.gen_range(1..10) {
            let opts = generate_random_service();
            let file_name = format!("{}.yaml", opts.name);
            let tmpfile = File::create(tmpdir.path().join(file_name)).unwrap();
            serde_yaml::to_writer(tmpfile, &opts).unwrap();
        }
        let read_result = read_all(tmpdir.path());
        assert!(read_result.is_ok());

        let file_name = "s1.someOtherExtension".to_string();
        let tmpfile = File::create(tmpdir.path().join(file_name)).unwrap();
        let opts = generate_random_service();
        serde_yaml::to_writer(tmpfile, &opts).unwrap();
        let read_result = read_all(tmpdir.path());
        assert!(read_result.is_err());
    }

    fn generate_random_service() -> ServiceOpts {
        let mut rng = thread_rng();
        let name: String = (&mut rng)
            .sample_iter(Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();

        let cmd: String = (&mut rng)
            .sample_iter(Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();

        let health_check: String = (&mut rng)
            .sample_iter(Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();

        let mut after = Vec::new();
        for _ in 0..rng.gen_range(1..10) {
            let s: String = (&mut rng)
                .sample_iter(Alphanumeric)
                .take(7)
                .map(char::from)
                .collect();
            after.push(s);
        }

        ServiceOpts {
            name,
            cmd,
            health_check,
            combine_log: rng.gen_bool(0.5),
            one_shot: rng.gen_bool(0.5),
            after,
        }
    }
}
