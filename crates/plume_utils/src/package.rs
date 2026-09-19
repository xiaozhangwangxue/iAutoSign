use super::{Bundle, PlistInfoTrait};
use crate::{Error, SignerApp, SignerOptions, cgbi};
use plist::Dictionary;
use std::path::PathBuf;
use std::{env, fs, io::Read};
use uuid::Uuid;
use zip::ZipArchive;
use zip::write::FileOptions;

#[derive(Debug, Clone)]
pub struct Package {
    package_file: PathBuf,
    stage_dir: PathBuf,
    stage_payload_dir: PathBuf,
    info_plist_dictionary: Dictionary,
    archive_entries: Vec<String>,
    pub app_icon_data: Option<Vec<u8>>,
}

impl Package {
    pub fn new(package_file: PathBuf) -> Result<Self, Error> {
        // Keep this short on purpose: the whole IPA is unpacked below it and
        // Windows refuses paths longer than 260 characters unless they use the
        // `\\?\` prefix. Every character saved here is available to the app
        // bundle's own file names.
        let id = Uuid::new_v4().simple().to_string();
        let stage_dir = env::temp_dir().join(format!("plume_{}", &id[..8]));
        let out_package_file = stage_dir.join("stage.ipa");

        fs::create_dir_all(&stage_dir).ok();
        fs::copy(&package_file, &out_package_file)?;

        let file = fs::File::open(&out_package_file)?;
        let mut archive = ZipArchive::new(file)?;
        let archive_entries = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
            .collect::<Vec<_>>();

        let info_plist_dictionary =
            Self::get_info_plist_from_archive(&out_package_file, &archive_entries)?;

        let app_icon_data = Self::extract_icon_from_archive(
            &out_package_file,
            &archive_entries,
            &info_plist_dictionary,
        );

        Ok(Self {
            package_file: out_package_file,
            stage_dir: stage_dir.clone(),
            stage_payload_dir: stage_dir.join("Payload"),
            info_plist_dictionary,
            archive_entries,
            app_icon_data,
        })
    }

    pub fn package_file(&self) -> &PathBuf {
        &self.package_file
    }

    fn get_info_plist_from_archive(
        archive_path: &PathBuf,
        archive_entries: &[String],
    ) -> Result<Dictionary, Error> {
        let file = fs::File::open(archive_path)?;
        let mut archive = ZipArchive::new(file)?;

        let info_plist_path = archive_entries
            .iter()
            .find(|entry| {
                entry.starts_with("Payload/")
                    && entry.ends_with("/Info.plist")
                    && entry.matches('/').count() == 2
            })
            .ok_or(Error::PackageInfoPlistMissing)?;

        let mut plist_file = archive.by_name(info_plist_path)?;
        let mut plist_data = Vec::new();
        plist_file.read_to_end(&mut plist_data)?;

        Ok(plist::from_bytes(&plist_data)?)
    }

    fn extract_icon_from_archive(
        archive_path: &PathBuf,
        archive_entries: &[String],
        plist: &Dictionary,
    ) -> Option<Vec<u8>> {
        // Collects all candidate icon base names from the plist, in order of preference.
        // CFBundleIcons (iPhone) takes priority, fall back to CFBundleIcons~ipad, then
        // top-level CFBundleIconFiles.
        let mut icon_names: Vec<String> = Vec::new();

        let primary_from = |d: &Dictionary| -> Vec<String> {
            d.get("CFBundlePrimaryIcon")
                .and_then(|v| v.as_dictionary())
                .and_then(|d| d.get("CFBundleIconFiles"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_string())
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };

        if let Some(d) = plist.get("CFBundleIcons").and_then(|v| v.as_dictionary()) {
            icon_names.extend(primary_from(d));
        }
        if let Some(d) = plist
            .get("CFBundleIcons~ipad")
            .and_then(|v| v.as_dictionary())
        {
            for n in primary_from(d) {
                if !icon_names.contains(&n) {
                    icon_names.push(n);
                }
            }
        }
        if let Some(arr) = plist.get("CFBundleIconFiles").and_then(|v| v.as_array()) {
            for n in arr
                .iter()
                .filter_map(|v| v.as_string())
                .map(|s| s.to_string())
            {
                if !icon_names.contains(&n) {
                    icon_names.push(n);
                }
            }
        }

        if icon_names.is_empty() {
            return None;
        }

        let app_prefix = archive_entries
            .iter()
            .find(|e| {
                e.starts_with("Payload/")
                    && e.ends_with("/Info.plist")
                    && e.matches('/').count() == 2
            })?
            .trim_end_matches("/Info.plist");

        let file = fs::File::open(archive_path).ok()?;
        let mut archive = ZipArchive::new(file).ok()?;

        let suffixes = ["@3x.png", "@2x.png", "@1x.png", ".png"];

        for name in &icon_names {
            for suffix in &suffixes {
                let candidate = format!("{app_prefix}/{name}{suffix}");
                if let Ok(mut entry) = archive.by_name(&candidate) {
                    let mut data = Vec::new();
                    if entry.read_to_end(&mut data).is_ok() && !data.is_empty() {
                        return Some(cgbi::normalize(data));
                    }
                }
            }
        }

        None
    }

    pub fn get_package_bundle(&self) -> Result<Bundle, Error> {
        let file = fs::File::open(&self.package_file)?;
        let mut archive = ZipArchive::new(file)?;
        archive.extract(&Self::extraction_path(&self.stage_dir))?;

        let app_dir = fs::read_dir(&self.stage_payload_dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.is_dir() && p.extension().and_then(|e| e.to_str()) == Some("app"))
            .ok_or_else(|| Error::PackageInfoPlistMissing)?;

        Ok(Bundle::new(app_dir)?)
    }

    /// Path used when unpacking the archive.
    ///
    /// iOS app bundles contain deeply nested frameworks, so the unpacked tree
    /// can easily exceed the Windows `MAX_PATH` limit. Windows accepts longer
    /// paths when they start with `\\?\`, which is what this adds.
    fn extraction_path(path: &std::path::Path) -> PathBuf {
        #[cfg(windows)]
        {
            if let Ok(canonical) = path.canonicalize() {
                let text = canonical.to_string_lossy().to_string();
                if !text.starts_with(r"\\?\") {
                    return PathBuf::from(format!(r"\\?\{text}"));
                }
                return PathBuf::from(text);
            }
        }

        path.to_path_buf()
    }

    pub fn get_archive_based_on_path(&self, path: &PathBuf) -> Result<PathBuf, Error> {
        if path.is_dir() {
            self.clone().archive_package_bundle()
        } else {
            Ok(self.package_file.clone())
        }
    }

    fn archive_package_bundle(self) -> Result<PathBuf, Error> {
        let zip_file_path = self.stage_dir.join("resigned.ipa");
        let file = fs::File::create(&zip_file_path)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        let payload_dir = self.stage_payload_dir;

        fn add_dir_to_zip(
            zip: &mut zip::ZipWriter<fs::File>,
            path: &PathBuf,
            prefix: &PathBuf,
            options: &FileOptions<'_, zip::write::ExtendedFileOptions>,
        ) -> Result<(), Error> {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let entry_path = entry.path();
                let name = entry_path
                    .strip_prefix(prefix)
                    .map_err(|_| Error::PackageInfoPlistMissing)?
                    .to_string_lossy()
                    .replace('\\', "/");

                if entry_path.is_file() {
                    zip.start_file(&name, options.clone())?;
                    let mut f = fs::File::open(&entry_path)?;
                    std::io::copy(&mut f, zip)?;
                } else if entry_path.is_dir() {
                    zip.add_directory(&name, options.clone())?;
                    add_dir_to_zip(zip, &entry_path, prefix, options)?;
                }
            }
            Ok(())
        }

        add_dir_to_zip(&mut zip, &payload_dir, &self.stage_dir, &options)?;
        zip.finish()?;

        Ok(zip_file_path)
    }

    pub fn remove_package_stage(self) {
        fs::remove_dir_all(&self.stage_dir).ok();
    }
}

// TODO: make bundle and package share a common trait for plist info access
macro_rules! get_plist_dict_value {
    ($self:ident, $key:expr) => {{
        $self
            .info_plist_dictionary
            .get($key)
            .and_then(|v| v.as_string())
            .map(|s| s.to_string())
    }};
}

impl PlistInfoTrait for Package {
    fn get_name(&self) -> Option<String> {
        get_plist_dict_value!(self, "CFBundleDisplayName")
            .or_else(|| get_plist_dict_value!(self, "CFBundleName"))
            .or_else(|| self.get_executable())
    }

    fn get_executable(&self) -> Option<String> {
        get_plist_dict_value!(self, "CFBundleExecutable")
    }

    fn get_bundle_identifier(&self) -> Option<String> {
        get_plist_dict_value!(self, "CFBundleIdentifier")
    }

    fn get_bundle_name(&self) -> Option<String> {
        get_plist_dict_value!(self, "CFBundleName")
    }

    fn get_version(&self) -> Option<String> {
        get_plist_dict_value!(self, "CFBundleShortVersionString")
    }

    fn get_build_version(&self) -> Option<String> {
        get_plist_dict_value!(self, "CFBundleVersion")
    }
}

impl Package {
    pub fn load_into_signer_options<'settings, 'slf: 'settings>(
        &'slf self,
        settings: &'settings mut SignerOptions,
    ) {
        let app = if self
            .archive_entries
            .iter()
            .any(|entry| entry.contains("SideStoreApp.framework"))
        {
            SignerApp::LiveContainerAndSideStore
        } else {
            SignerApp::from_bundle_identifier(self.get_bundle_identifier().as_deref())
        };

        let new_settings = SignerOptions::new_for_app(app);
        *settings = new_settings;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Builds a minimal but valid IPA on disk.
    fn build_test_ipa(directory: &std::path::Path) -> PathBuf {
        let path = directory.join("Test.ipa");
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();

        let mut plist = plist::Dictionary::new();
        plist.insert(
            "CFBundleIdentifier".to_string(),
            plist::Value::String("com.example.autosign-test".to_string()),
        );
        plist.insert(
            "CFBundleExecutable".to_string(),
            plist::Value::String("TestApp".to_string()),
        );
        plist.insert(
            "CFBundleName".to_string(),
            plist::Value::String("AutoSign Test".to_string()),
        );

        let mut plist_bytes = Vec::new();
        plist::to_writer_xml(&mut plist_bytes, &plist).unwrap();

        zip.start_file("Payload/Test.app/Info.plist", options)
            .unwrap();
        zip.write_all(&plist_bytes).unwrap();

        zip.start_file("Payload/Test.app/TestApp", options).unwrap();
        zip.write_all(b"not a real Mach-O").unwrap();

        // A nested directory, like the frameworks a real app ships.
        zip.add_directory("Payload/Test.app/Frameworks/", options)
            .unwrap();
        zip.start_file("Payload/Test.app/Frameworks/Lib.framework/Lib", options)
            .unwrap();
        zip.write_all(b"framework payload").unwrap();

        zip.finish().unwrap();
        path
    }

    fn temp_dir() -> PathBuf {
        let id = Uuid::new_v4().simple().to_string();
        let directory = env::temp_dir().join(format!("plume_test_{}", &id[..8]));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn unpacks_an_ipa_into_a_usable_bundle() {
        let directory = temp_dir();
        let ipa = build_test_ipa(&directory);

        let package = Package::new(ipa).unwrap();
        let bundle = package.get_package_bundle().unwrap();

        assert!(bundle.bundle_dir().join("Info.plist").exists());
        assert!(bundle.bundle_dir().join("TestApp").exists());
        assert!(
            bundle
                .bundle_dir()
                .join("Frameworks/Lib.framework/Lib")
                .exists()
        );

        assert_eq!(
            package.get_bundle_identifier().as_deref(),
            Some("com.example.autosign-test")
        );

        package.remove_package_stage();
        fs::remove_dir_all(&directory).ok();
    }

    /// Windows shows "messy" archives when entries are absolute or use
    /// backslashes; the repacked IPA must stay a normal, relative archive.
    #[test]
    fn repacked_ipa_uses_relative_forward_slash_paths() {
        let directory = temp_dir();
        let ipa = build_test_ipa(&directory);

        let package = Package::new(ipa).unwrap();
        let bundle = package.get_package_bundle().unwrap();
        let repacked = package
            .get_archive_based_on_path(&bundle.bundle_dir().clone())
            .unwrap();

        let file = fs::File::open(&repacked).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();

        let mut names = Vec::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index).unwrap();
            names.push(entry.name().to_string());
        }

        assert!(!names.is_empty());
        for name in &names {
            assert!(
                !name.contains('\\'),
                "entry uses a Windows separator: {name}"
            );
            assert!(!name.starts_with('/'), "entry is absolute: {name}");
            assert!(
                name.starts_with("Payload/"),
                "entry is not relative to Payload: {name}"
            );
        }
        assert!(names.iter().any(|n| n.ends_with("Info.plist")));

        package.remove_package_stage();
        fs::remove_dir_all(&directory).ok();
    }
}
