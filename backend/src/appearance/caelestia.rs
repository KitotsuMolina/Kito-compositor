use super::{AppearanceOperation, CaelestiaSnapshot};
use crate::ProcessExecutor;
use std::path::{Path, PathBuf};

pub fn apply_operations(image: &Path, activate_dynamic: bool) -> Vec<AppearanceOperation> {
    let wallpaper = operation(vec![
        "wallpaper".into(),
        "--file".into(),
        image.to_string_lossy().into_owned(),
    ]);
    if activate_dynamic {
        vec![
            wallpaper,
            operation(strings(&["scheme", "set", "--name", "dynamic"])),
        ]
    } else {
        vec![wallpaper]
    }
}

pub fn restore_operations(snapshot: &CaelestiaSnapshot) -> Vec<AppearanceOperation> {
    let scheme = operation(vec![
        "scheme".into(),
        "set".into(),
        "--name".into(),
        snapshot.name.clone(),
        "--flavour".into(),
        snapshot.flavour.clone(),
        "--mode".into(),
        snapshot.mode.clone(),
        "--variant".into(),
        snapshot.variant.clone(),
    ]);
    let wallpaper = snapshot.wallpaper.as_ref().map(|path| {
        operation(vec![
            "wallpaper".into(),
            "--file".into(),
            path.to_string_lossy().into_owned(),
        ])
    });
    if snapshot.name == "dynamic" {
        wallpaper.into_iter().chain([scheme]).collect()
    } else {
        [scheme].into_iter().chain(wallpaper).collect()
    }
}

pub fn snapshot<E: ProcessExecutor>(executor: &E) -> Result<CaelestiaSnapshot, String> {
    let wallpaper = current_wallpaper(executor)?;
    Ok(CaelestiaSnapshot {
        name: query(executor, &["scheme", "get", "--name"])?,
        flavour: query(executor, &["scheme", "get", "--flavour"])?,
        mode: query(executor, &["scheme", "get", "--mode"])?,
        variant: query(executor, &["scheme", "get", "--variant"])?,
        wallpaper,
    })
}

pub fn current_name<E: ProcessExecutor>(executor: &E) -> Result<String, String> {
    query(executor, &["scheme", "get", "--name"])
}

pub fn current_wallpaper<E: ProcessExecutor>(executor: &E) -> Result<Option<PathBuf>, String> {
    let value = query(executor, &["wallpaper"])?;
    Ok((value != "No wallpaper set")
        .then(|| PathBuf::from(value))
        .filter(|path| path.is_absolute()))
}

pub fn execute<E: ProcessExecutor>(
    executor: &E,
    operations: &[AppearanceOperation],
) -> Result<(), String> {
    for operation in operations {
        executor.run(&operation.binary, &operation.args)?;
    }
    Ok(())
}

fn query<E: ProcessExecutor>(executor: &E, args: &[&str]) -> Result<String, String> {
    let args = args.iter().map(|value| (*value).into()).collect::<Vec<_>>();
    let output = executor.run("caelestia", &args)?;
    if output.stdout.trim().is_empty() {
        return Err(format!(
            "caelestia returned an empty value for {}",
            args.join(" ")
        ));
    }
    Ok(output.stdout.trim().into())
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

fn operation(args: Vec<String>) -> AppearanceOperation {
    AppearanceOperation {
        binary: "caelestia".into(),
        args,
        mutates: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activates_dynamic_scheme_when_required() {
        let operations = apply_operations(Path::new("/tmp/wallpaper.jpg"), true);

        assert_eq!(operations.len(), 2);
        assert_eq!(
            operations[1].args,
            strings(&["scheme", "set", "--name", "dynamic"])
        );
    }

    #[test]
    fn changing_wallpaper_does_not_reapply_an_active_dynamic_scheme() {
        let operations = apply_operations(Path::new("/tmp/wallpaper.jpg"), false);

        assert_eq!(operations.len(), 1);
        assert_eq!(
            operations[0].args,
            strings(&["wallpaper", "--file", "/tmp/wallpaper.jpg"])
        );
    }
}
