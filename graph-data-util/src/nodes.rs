pub mod downloader {
    use crate::command::download_nodes::DownloadNodes;
    use crate::nodes::persistence::Persistence;
    use crate::prelude::*;
    use clap::arg_enum;
    use std::hash::Hash;

    pub struct Downloader {
        pub options: DownloadNodes,
        pub persistence: Persistence,
    }

    arg_enum! {
        #[derive(Debug, PartialEq, Eq, Hash, Clone)]
        pub enum DownloadMode {
            VerifyExistingOnly,
            VerifyExistingAddNew,
            AddNew,
            AddNewOverwriteExisting,
        }
    }

    impl Downloader {
        pub async fn download(&mut self) -> crate::Result<()> {
            use graph_builder::registry::{cache, Registry};

            let registry = Registry::try_from_str(&self.options.registry)
                .map_err(|e| Error::msg(e.to_string()))
                .context(format!(
                    "Failed to parse registry '{}'",
                    &self.options.registry
                ))?;

            let mut cache = match &self.options.persistence_mode {
                // Download everything in verification modes
                DownloadMode::VerifyExistingOnly | DownloadMode::VerifyExistingAddNew => {
                    cache::Cache::new()
                }
                _ => self.persistence.get_cache().clone(),
            };

            let releases = graph_builder::registry::fetch_releases(
                // registry: &Registry,
                &registry,
                // repo: &str,
                &self.options.repository,
                // username: Option<&str>,
                self.options.username.as_deref(),
                // password: Option<&str>,
                self.options.password.as_deref(),
                // cache: &mut crate::registry::cache::Cache,
                &mut cache,
                // manifestref_manifestref: &str,
                &self.options.manifestref_key,
                // concurrency: usize
                self.options.concurrency,
            )
            .await
            .map_err(|e| Error::msg(e.to_string()))?;

            self.persistence
                .update_with(&releases.into_iter().try_fold(
                    cache::Cache::new(),
                    |mut collection, release| -> crate::Result<cache::Cache> {
                        let manifestref = release
                            .metadata
                            .metadata
                            .get(&self.options.manifestref_key)
                            .ok_or_else(|| {
                                Error::msg(format!(
                                    "Could not find metadata at '{}' in release '{:?}'",
                                    self.options.manifestref_key, &release
                                ))
                            })?
                            .clone();
                        collection.insert(manifestref, Some(release));
                        Ok(collection)
                    },
                )?)?;

            Ok(())
        }
    }
}

pub mod persistence {
    use crate::nodes::downloader::DownloadMode;
    use graph_builder::registry::cache::Key;
    use graph_builder::registry::Release;
    use std::collections::HashSet;
    use std::convert::TryInto;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    use crate::prelude::*;
    use graph_builder::registry::cache::Cache;

    pub struct Persistence {
        directory: PathBuf,
        mode: DownloadMode,
        cache: Cache,
    }

    struct Converter<T>(T);

    impl std::convert::TryInto<String> for Converter<&PathBuf> {
        type Error = crate::Error;

        fn try_into(self) -> crate::Result<String> {
            Converter(self.0.file_name()).try_into()
        }
    }

    impl std::convert::TryInto<String> for Converter<Option<&OsStr>> {
        type Error = crate::Error;

        fn try_into(self) -> crate::Result<String> {
            Ok(self
                .0
                .ok_or_else(|| Error::msg(format!("could not get file name from {:?}", &self.0)))?
                .to_str()
                .ok_or_else(|| Error::msg(format!("to_str() failed on {:?} into String", &self.0)))?
                .to_string())
        }
    }

    impl Persistence {
        pub fn get_cache(&self) -> &Cache {
            &self.cache
        }

        pub fn get_cache_mut(&mut self) -> &mut Cache {
            &mut self.cache
        }

        pub fn new(directory: PathBuf, mode: DownloadMode) -> crate::Result<Self> {
            let cache = Cache::new();

            let mut persistence = Self {
                directory,
                mode,
                cache,
            };

            if persistence.mode != DownloadMode::AddNewOverwriteExisting
                && persistence.directory.is_dir()
            {
                persistence.load_values()?;
            }

            Ok(persistence)
        }

        fn load_values(&mut self) -> crate::Result<()> {
            debug!("Populating cache from directory '{:?}'", self.directory);

            let algorithm_directories = std::fs::read_dir(&self.directory)?
                .filter_map(std::result::Result::ok)
                .map(|value| value.path())
                .filter(|path| path.is_dir());

            algorithm_directories
                .map(|algo_dir| -> crate::Result<_> {
                    let files = std::fs::read_dir(&algo_dir)?
                        .filter_map(std::result::Result::ok)
                        .map(|value| value.path())
                        .filter(|path| path.is_file())
                        .collect::<Vec<_>>();

                    let algo_dirname: String = Converter(&algo_dir).try_into()?;

                    Ok((algo_dirname, files))
                })
                .filter_map(|result| match result {
                    Ok(values) => Some(values),
                    Err(e) => {
                        warn!("{}", e);
                        None
                    }
                })
                .try_for_each(|(algo, files)| -> crate::Result<()> {
                    files.iter().try_for_each(|filepath| {
                        let file = std::fs::OpenOptions::new()
                            .create(false)
                            .create_new(false)
                            .read(true)
                            .open(&filepath)
                            .context(format!("[{:?}] Opening", &filepath))?;

                        let filename: String = Converter(filepath).try_into()?;

                        let release: Option<Release> = serde_json::from_reader(&file)
                            .context(format!("[{:?}] Deserialization to Release", &filename))?;

                        self.cache
                            .insert(format!("{}:{}", &algo, &filename), release);

                        Ok(())
                    })
                })?;

            info!(
                "Loaded {} values from directory '{:?}'",
                self.cache.len(),
                self.directory,
            );

            Ok(())
        }

        /// Persists all values in the current cache in an overwriting fashion.
        fn persist_value(&mut self, manifestref: &str) -> crate::Result<()> {
            let value = self
                .cache
                .get(manifestref)
                .ok_or_else(|| Error::msg(format!("[{}] Cache value missing", &manifestref)))?;

            let (algo, hash) = {
                let mut split = manifestref.split(':');
                match (split.next(), split.next(), split.next()) {
                    (Some(algo), Some(hash), None) => (algo, hash),
                    _ => bail!(
                        "[{}] manifestref is not in algo:hash format: {:?}",
                        &manifestref,
                        split
                    ),
                }
            };

            let algo_dir = self.directory.join(algo);
            std::fs::create_dir_all(&algo_dir).context(format!("Creating {:?}", algo_dir))?;

            let filepath = algo_dir.join(hash);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&filepath)
                .context(format!("[{}] Opening {:?}", &manifestref, &filepath))?;

            trace!("[{}] Persisting", &manifestref);

            serde_json::to_writer(&file, value)
                .map_err(Error::from)
                .context(format!(
                    "[{}] Failed to write to {:?}",
                    &manifestref, &filepath
                ))?;

            Ok(())
        }

        /// Verify that the common values of self.cache and the updated_cache are identical
        fn verify(&self, updated_cache: &Cache) -> crate::Result<()> {
            let mut differ_from_update = HashSet::<&Key>::new();
            let mut error: Option<Error> = None;

            for (updated_cache_manifestref, updated_cache_value) in updated_cache {
                if let Some(cache_value) = self.cache.get(updated_cache_manifestref) {
                    if cache_value != updated_cache_value {
                        differ_from_update.insert(updated_cache_manifestref);

                        let current_error_msg = format!(
                            "[{}] value mismatch.\ncached: '{:?}'\nudpated: '{:?}'",
                            updated_cache_manifestref, cache_value, updated_cache_value
                        );

                        error = Some(error.map_or_else(
                            || Error::msg(current_error_msg.to_owned()),
                            |e| e.context(current_error_msg.to_owned()),
                        ));
                    }
                }
            }

            if let Some(error) = error {
                error!(
                    "{} different entries in update: {:?}",
                    differ_from_update.len(),
                    differ_from_update
                );
                return Err(error);
            }

            info!("Verification successful.");
            Ok(())
        }

        pub fn update_with(&mut self, updated_cache: &Cache) -> crate::Result<()> {
            match &self.mode {
                DownloadMode::VerifyExistingOnly => {
                    return self.verify(&updated_cache);
                }
                DownloadMode::VerifyExistingAddNew => {
                    self.verify(&updated_cache)?;
                }

                _ => (),
            };

            updated_cache.iter().try_for_each(
                |(manifestref, updated_value)| -> crate::Result<()> {
                    trace!("[{}] Processing.", &manifestref,);

                    match &self.mode {
                        DownloadMode::VerifyExistingOnly => {
                            // This was handled before the match statement
                            unreachable!()
                        }
                        DownloadMode::AddNew => {
                            self.cache
                                .insert(manifestref.to_string(), updated_value.to_owned());

                            self.persist_value(manifestref)?;
                        }
                        DownloadMode::VerifyExistingAddNew => {
                            // Ensure any existing manifestrefs match the updated value
                            if let Some(value) = self.cache.get(manifestref) {
                                trace!("value exists.");
                                assert_eq!(value, updated_value);
                                ensure!(
                                    value == updated_value,
                                    "[{}] Cached value '{:?}' != updated value '{:?}'",
                                    manifestref,
                                    value,
                                    updated_value
                                );
                            } else {
                                trace!("value doesn't exist in {:?}", self.cache.keys());
                                self.cache
                                    .insert(manifestref.to_string(), updated_value.to_owned());

                                self.persist_value(manifestref)?;
                            }
                        }
                        DownloadMode::AddNewOverwriteExisting => {
                            // Overwrite any existing manifestref with the updated value
                            self.cache
                                .insert(manifestref.to_string(), updated_value.to_owned());

                            self.persist_value(manifestref)?;
                        }
                    }

                    Ok(())
                },
            )?;

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;

    #[test]
    fn e2e_simple_repo() -> Result<()> {
        bail!("unimpml")
    }
}
