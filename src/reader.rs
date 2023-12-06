use crate::interface::Interface;

use regex::Regex;
use std::fs::{self, read_dir};
use std::path::{Path, PathBuf};

pub struct InterfacesResult {
    pub interfaces: Vec<Interface>,
    pub warning: Option<anyhow::Error>,
}

pub fn read_xml_file(path: PathBuf) -> Result<InterfacesResult, anyhow::Error> {
    let contents = match fs::read_to_string(path.clone()) {
        Ok(contents) => contents,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Couldn't read {}: {}",
                path.as_path().display(),
                e
            ))
        }
    };
    let replaced_string = replace_colons(contents.as_str());
    let deserializer = &mut quick_xml::de::Deserializer::from_str(replaced_string.as_str());
    let mut unhandled_fields = vec![];
    let interfaces: Vec<Interface> = serde_ignored::deserialize(deserializer, |path| {
        unhandled_fields.push(path.to_string());
    })?;
    let mut result = InterfacesResult {
        interfaces,
        warning: None,
    };
    if !unhandled_fields.is_empty() {
        for unused_str in unhandled_fields {
            let split_str = unused_str.split_once('.').unwrap();
            log::warn!(
                "Unhandled field in interface {}: {}",
                result.interfaces[split_str.0.parse::<usize>().unwrap()].name,
                split_str.1
            );
        }
        result.warning = Some(anyhow::anyhow!("Unhandled fields"))
    }
    Ok(result)
}

fn replace_colons(colon_string: &str) -> String {
    let re = Regex::new(r"<([\/]?)(\w+):(\w+)\b").unwrap();
    let replaced = re.replace_all(colon_string, "<$1$2-$3").to_string();
    replaced
}

// https://stackoverflow.com/a/76820878
fn recurse_files(path: impl AsRef<Path>) -> std::io::Result<Vec<PathBuf>> {
    let mut buf = vec![];
    let entries = read_dir(path)?;

    for entry in entries {
        let entry = entry?;
        let meta = entry.metadata()?;

        if meta.is_dir() {
            let mut subdir = recurse_files(entry.path())?;
            buf.append(&mut subdir);
        }

        if meta.is_file() {
            buf.push(entry.path());
        }
    }

    Ok(buf)
}

pub fn read(paths: Vec<String>) -> Result<InterfacesResult, anyhow::Error> {
    let mut result = InterfacesResult {
        interfaces: vec![],
        warning: None,
    };
    for path in paths {
        let path: PathBuf = path.into();
        if path.is_dir() {
            let files = recurse_files(path)?;
            for file in files {
                let mut read_xml = read_xml_file(file)?;
                if result.warning.is_none() && read_xml.warning.is_some() {
                    result.warning = read_xml.warning
                }
                result.interfaces.append(&mut read_xml.interfaces);
            }
        } else {
            let mut read_xml = read_xml_file(path)?;
            if result.warning.is_none() && read_xml.warning.is_some() {
                result.warning = read_xml.warning
            }
            result.interfaces.append(&mut read_xml.interfaces);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interface::*;

    #[test]
    fn test_broken_xml() {
        let xml = r##"
            <interface>
                <name>eth1</name>
                <ipv4:static>
                  <address>127.0.0.1</>
                </ipv4:static>
            </interface>
            "##;
        let err = quick_xml::de::from_str::<Vec<Interface>>(replace_colons(xml).as_str());
        assert!(err.is_err());
    }
}
