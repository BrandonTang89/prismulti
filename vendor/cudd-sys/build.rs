extern crate autotools;

use SHA256Status::{Mismatch, Unknown};
use autotools::Config;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

const PACKAGE_URL: &str =
    "https://github.com/cuddorg/cudd/releases/download/3.0.0/cudd-3.0.0.tar.gz";
const PACKAGE_SHA256: &str = "5fe145041c594689e6e7cf4cd623d5f2b7c36261708be8c9a72aed72cf67acce";

#[derive(Debug)]
#[allow(dead_code)]
enum FetchError {
    CommandError(std::process::ExitStatus),
    IOError(std::io::Error),
    PathExists,
}

enum SHA256Status {
    Match,
    Mismatch,
    Unknown,
}

impl From<std::io::Error> for FetchError {
    fn from(err: std::io::Error) -> FetchError {
        FetchError::IOError(err)
    }
}

/// Run a command and return (stdout, stderr) if exit status is success.
fn run_command(cmd: &mut Command) -> Result<(String, String), FetchError> {
    let output = cmd.output()?;

    if output.status.success() {
        Ok((
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap(),
        ))
    } else {
        eprintln!("Command {:?} exited with status {}", cmd, output.status);
        Err(FetchError::CommandError(output.status))
    }
}

fn replace_once(contents: &str, from: &str, to: &str, context: &str) -> Result<String, String> {
    if !contents.contains(from) {
        return Err(format!("Could not find patch anchor for {context}."));
    }
    Ok(contents.replacen(from, to, 1))
}

fn patch_cudd_sources(cudd_path: &Path) -> Result<(), String> {
    let cudd_h_path = cudd_path.join("cudd/cudd.h");
    let cudd_int_h_path = cudd_path.join("cudd/cuddInt.h");
    let cudd_add_abs_path = cudd_path.join("cudd/cuddAddAbs.c");

    let mut cudd_h = fs::read_to_string(&cudd_h_path)
        .map_err(|e| format!("Cannot read {}: {e}", cudd_h_path.display()))?;
    if !cudd_h.contains("Cudd_addMaxAbstract") {
        cudd_h = replace_once(
            &cudd_h,
            "extern DdNode * Cudd_addOrAbstract(DdManager *manager, DdNode *f, DdNode *cube);\n",
            "extern DdNode * Cudd_addOrAbstract(DdManager *manager, DdNode *f, DdNode *cube);\nextern DdNode * Cudd_addMaxAbstract(DdManager *manager, DdNode *f, DdNode *cube);\n",
            "Cudd_addMaxAbstract declaration in cudd.h",
        )?;
        fs::write(&cudd_h_path, cudd_h)
            .map_err(|e| format!("Cannot write {}: {e}", cudd_h_path.display()))?;
    }

    let mut cudd_int_h = fs::read_to_string(&cudd_int_h_path)
        .map_err(|e| format!("Cannot read {}: {e}", cudd_int_h_path.display()))?;
    if !cudd_int_h.contains("cuddAddMaxAbstractRecur") {
        cudd_int_h = replace_once(
            &cudd_int_h,
            "extern DdNode * cuddAddOrAbstractRecur(DdManager *manager, DdNode *f, DdNode *cube);\n",
            "extern DdNode * cuddAddOrAbstractRecur(DdManager *manager, DdNode *f, DdNode *cube);\nextern DdNode * cuddAddMaxAbstractRecur(DdManager *manager, DdNode *f, DdNode *cube);\n",
            "cuddAddMaxAbstractRecur declaration in cuddInt.h",
        )?;
        fs::write(&cudd_int_h_path, cudd_int_h)
            .map_err(|e| format!("Cannot write {}: {e}", cudd_int_h_path.display()))?;
    }

    let mut cudd_add_abs = fs::read_to_string(&cudd_add_abs_path)
        .map_err(|e| format!("Cannot read {}: {e}", cudd_add_abs_path.display()))?;
    if !cudd_add_abs.contains("Cudd_addMaxAbstract(") {
        cudd_add_abs = replace_once(
            &cudd_add_abs,
            "} /* end of Cudd_addOrAbstract */\n\n\n/*---------------------------------------------------------------------------*/\n/* Definition of internal functions                                          */\n/*---------------------------------------------------------------------------*/\n",
            "} /* end of Cudd_addOrAbstract */\n\n\n/**\n  @brief Maximization-abstracts all the variables in cube from %ADD f.\n\n  @details Abstracts all the variables in cube from f by taking the\n  maximum over all values taken by the abstracted variables.\n\n  @return the abstracted %ADD if successful; NULL otherwise.\n\n  @sideeffect None\n\n  @see Cudd_addExistAbstract Cudd_addUnivAbstract Cudd_addOrAbstract\n\n*/\nDdNode *\nCudd_addMaxAbstract(\n  DdManager * manager,\n  DdNode * f,\n  DdNode * cube)\n{\n    DdNode *res;\n\n    if (addCheckPositiveCube(manager, cube) == 0) {\n        (void) fprintf(manager->err,\"Error: Can only abstract cubes\");\n        return(NULL);\n    }\n\n    do {\n        manager->reordered = 0;\n        res = cuddAddMaxAbstractRecur(manager, f, cube);\n    } while (manager->reordered == 1);\n    if (manager->errorCode == CUDD_TIMEOUT_EXPIRED && manager->timeoutHandler) {\n        manager->timeoutHandler(manager, manager->tohArg);\n    }\n\n    return(res);\n\n} /* end of Cudd_addMaxAbstract */\n\n\n/*---------------------------------------------------------------------------*/\n/* Definition of internal functions                                          */\n/*---------------------------------------------------------------------------*/\n",
            "Cudd_addMaxAbstract exported function in cuddAddAbs.c",
        )?;

        cudd_add_abs = replace_once(
            &cudd_add_abs,
            "} /* end of cuddAddOrAbstractRecur */\n\n\n\n/*---------------------------------------------------------------------------*/\n/* Definition of static functions                                            */\n/*---------------------------------------------------------------------------*/\n",
            "} /* end of cuddAddOrAbstractRecur */\n\n\n/**\n  @brief Performs the recursive step of Cudd_addMaxAbstract.\n\n  @return the %ADD obtained by abstracting the variables of cube from\n  f with maximization, if successful; NULL otherwise.\n\n  @sideeffect None\n\n*/\nDdNode *\ncuddAddMaxAbstractRecur(\n  DdManager * manager,\n  DdNode * f,\n  DdNode * cube)\n{\n    DdNode *T, *E, *res, *res1, *res2, *one;\n\n    statLine(manager);\n    one = DD_ONE(manager);\n\n    /* Cube is guaranteed to be a cube at this point. */\n    if (cuddIsConstant(f) || cube == one) {\n        return(f);\n    }\n\n    /* Abstract a variable that does not appear in f. */\n    if (cuddI(manager,f->index) > cuddI(manager,cube->index)) {\n        return(cuddAddMaxAbstractRecur(manager, f, cuddT(cube)));\n    }\n\n    if ((res = cuddCacheLookup2(manager, Cudd_addMaxAbstract, f, cube)) != NULL) {\n        return(res);\n    }\n\n    checkWhetherToGiveUp(manager);\n\n    T = cuddT(f);\n    E = cuddE(f);\n\n    /* If the two indices are the same, so are their levels. */\n    if (f->index == cube->index) {\n        res1 = cuddAddMaxAbstractRecur(manager, T, cuddT(cube));\n        if (res1 == NULL) return(NULL);\n        cuddRef(res1);\n        res2 = cuddAddMaxAbstractRecur(manager, E, cuddT(cube));\n        if (res2 == NULL) {\n            Cudd_RecursiveDeref(manager,res1);\n            return(NULL);\n        }\n        cuddRef(res2);\n        res = cuddAddApplyRecur(manager, Cudd_addMaximum, res1, res2);\n        if (res == NULL) {\n            Cudd_RecursiveDeref(manager,res1);\n            Cudd_RecursiveDeref(manager,res2);\n            return(NULL);\n        }\n        cuddRef(res);\n        Cudd_RecursiveDeref(manager,res1);\n        Cudd_RecursiveDeref(manager,res2);\n        cuddCacheInsert2(manager, Cudd_addMaxAbstract, f, cube, res);\n        cuddDeref(res);\n        return(res);\n    } else { /* if (cuddI(manager,f->index) < cuddI(manager,cube->index)) */\n        res1 = cuddAddMaxAbstractRecur(manager, T, cube);\n        if (res1 == NULL) return(NULL);\n        cuddRef(res1);\n        res2 = cuddAddMaxAbstractRecur(manager, E, cube);\n        if (res2 == NULL) {\n            Cudd_RecursiveDeref(manager,res1);\n            return(NULL);\n        }\n        cuddRef(res2);\n        res = (res1 == res2) ? res1 :\n            cuddUniqueInter(manager, (int) f->index, res1, res2);\n        if (res == NULL) {\n            Cudd_RecursiveDeref(manager,res1);\n            Cudd_RecursiveDeref(manager,res2);\n            return(NULL);\n        }\n        cuddDeref(res1);\n        cuddDeref(res2);\n        cuddCacheInsert2(manager, Cudd_addMaxAbstract, f, cube, res);\n        return(res);\n    }\n\n} /* end of cuddAddMaxAbstractRecur */\n\n\n/*---------------------------------------------------------------------------*/\n/* Definition of static functions                                            */\n/*---------------------------------------------------------------------------*/\n",
            "cuddAddMaxAbstractRecur implementation in cuddAddAbs.c",
        )?;

        fs::write(&cudd_add_abs_path, cudd_add_abs)
            .map_err(|e| format!("Cannot write {}: {e}", cudd_add_abs_path.display()))?;
    }

    Ok(())
}

/// Fetch a file from a URL if it does not already exist in out_dir and verify its sha256sum if possible.
fn fetch_package(
    out_dir: &str,
    url: &str,
    sha256: &str,
) -> Result<(PathBuf, SHA256Status), FetchError> {
    let out_path = Path::new(&out_dir);
    let target_path = out_path.join(Path::new(url).file_name().unwrap());
    let target_path_str = target_path.clone().into_os_string().into_string().unwrap();

    match target_path.metadata() {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            // Path does not exist! Start download...
            println!("Downloading {} to {}", url, target_path_str);
            let mut command = Command::new("curl");
            command.args(["-L", url, "-o", target_path_str.as_str()]);
            run_command(&mut command)?;
        }
        Ok(data) if data.is_file() => {
            println!("{} exists. Skipping download.", target_path_str);
        }
        Ok(_) => return Err(FetchError::PathExists),
        Err(error) => return Err(FetchError::from(error)),
    }

    // Now run sha256 sum check:
    let mut command_1 = Command::new("sha256sum");
    command_1.arg(target_path.clone());
    let mut command_2 = Command::new("shasum -a 256");
    command_2.arg(target_path.clone());
    let sha256_result = run_command(&mut command_1).or_else(|_| run_command(&mut command_2));

    let sha256_status = match sha256_result {
        Err(_) => SHA256Status::Unknown,
        Ok((output, _)) if output.contains(sha256) => SHA256Status::Match,
        _ => SHA256Status::Mismatch,
    };

    Ok((target_path, sha256_status))
}

fn main() -> Result<(), String> {
    let build_cudd = env::var_os("CARGO_FEATURE_BUILD_CUDD").is_some();
    if !build_cudd {
        // If silent build is active, don't do anything.
        return Ok(());
    }

    let out_dir = env::var("OUT_DIR")
        .map_err(|_| "Environmental variable `OUT_DIR` not defined.".to_string())?;

    let (tar_path, sha256_status) = fetch_package(&out_dir, PACKAGE_URL, PACKAGE_SHA256)
        .map_err(|e| format!("Error downloading CUDD package: {:?}.", e))?;
    let tar_path_str = tar_path.to_str().unwrap().to_string();

    match sha256_status {
        Unknown => eprintln!("WARNING: SHA256 not computed. Package validation skipped."),
        Mismatch => return Err("CUDD package SHA256 hash mismatch.".to_string()),
        _ => (),
    }

    // Get cudd.tar.gz path without extensions.
    let cudd_path = tar_path.with_extension("").with_extension("");
    let cudd_path_str = cudd_path.clone().into_os_string().into_string().unwrap();

    if !cudd_path.exists() {
        // Create the destination directory.
        std::fs::create_dir_all(cudd_path.clone())
            .map_err(|e| format!("Cannot create CUDD directory: {:?}", e))?;
    }

    // un-tar package, ignoring the name of the top level folder, dumping into cudd_path instead.
    let mut tar_command = Command::new("tar");
    tar_command.args([
        "xf",
        &tar_path_str,
        "--strip-components=1",
        "-C",
        &cudd_path_str,
    ]);
    run_command(&mut tar_command).map_err(|e| format!("Error decompressing CUDD: {:?}", e))?;

    patch_cudd_sources(cudd_path.as_path())?;

    // Enable dddmp when building.
    let build_output = Config::new(cudd_path).enable("dddmp", None).build();

    println!(
        "cargo:rustc-link-search=native={}",
        build_output.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=cudd");

    Ok(())
}
