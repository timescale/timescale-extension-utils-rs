fn main() {
    #[cfg(feature = "parse_headers")]
    parse_headers::main()
}

#[cfg(feature = "parse_headers")]
mod parse_headers {
    //based on https://github.com/bluejekyll/pg-extend-rs/blob/a8d637ca83475905b4799fbd123455c97b949a4a/pg-extend/build.rs
    use std::{
        collections::HashSet,
        env,
        fs::OpenOptions,
        io::{BufWriter, Write},
        path::PathBuf,
        process::Command,
    };

    pub fn main() {
        let out_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("generated.rs");
        let pg_config = env::var("PG_CONFIG").unwrap_or_else(|_| "pg_config".to_string());

        // Re-run this if wrapper.h changes
        println!("cargo:rerun-if-changed=wrapper.h");

        let pg_include = include_dir(&pg_config)
            .expect(concat!("Could not find postgres install\n",
                "\teither set PG_INCLUDE_PATH to the Postgres install include dir, e.g. PG_INCLUDE_PATH=/var/lib/pgsql/include/server\n",
                "\tor set PG_CONFIG to the path to `pg_config`",
            ));

        // these cause duplicate definition problems on linux
        // see: https://github.com/rust-lang/rust-bindgen/issues/687
        let ignored_macros = IgnoreMacros(
            vec![
                "FP_INFINITE".into(),
                "FP_NAN".into(),
                "FP_NORMAL".into(),
                "FP_SUBNORMAL".into(),
                "FP_ZERO".into(),
                "IPPORT_RESERVED".into(),
            ]
            .into_iter()
            .collect(),
        );

        let bindings = get_bindings(&pg_include) // Gets initial bindings that are OS-dependant
            // The input header we would like to generate
            // bindings for.
            .header("wrapper.h")
            .parse_callbacks(Box::new(ignored_macros))
            .rustfmt_bindings(true)
            .raw_line(r##"#[cfg(target_os = "linux")] use std::os::raw::c_int;"##)
            .raw_line(r##"#[cfg(all(target_os = "linux", target_env = "gnu"))] use crate::sigsetjmp;"##)
            // this function causes a error: function parameters cannot shadow statics
            .blacklist_function("XLogReaderAllocate")
            // TODO: add this back?
            .layout_tests(false);

        // Finish the builder and generate the bindings.
        let bindings = bindings
            .generate()
            // Unwrap the Result and panic on failure.
            .expect("Unable to generate bindings");

        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(out_path)
            .expect("could not open bindings file");
        let mut file = BufWriter::new(file);
        file.write_all(b"#[pg_guard_function::pg_guard_function] pub mod bindgenerated {\n")
            .expect("cannot write mod");
        // Write the bindings to the $OUT_DIR/postgres.rs file.
        bindings
            .write(Box::new(&mut file))
            .expect("Couldn't write bindings!");
        file.write_all(b"}\n").expect("cannot write");
    }

    #[cfg(windows)]
    fn get_bindings(pg_include: &str) -> bindgen::Builder {
        // Compilation in windows requires these extra inclde paths
        let pg_include_win32_msvc = format!("{}\\port\\win32_msvc", pg_include);
        let pg_include_win32 = format!("{}\\port\\win32", pg_include);
        // The `pg_include` path comes in the format og "includes/server", but we also need
        // the parent folder, so we remove the "/server" part at the end
        let pg_include_parent = pg_include[..(pg_include.len() - 7)].to_owned();

        bindgen::Builder::default()
            .clang_arg(format!("-I{}", pg_include_win32_msvc))
            .clang_arg(format!("-I{}", pg_include_win32))
            .clang_arg(format!("-I{}", pg_include))
            .clang_arg(format!("-I{}", pg_include_parent))
            // Whitelist all PG-related functions
            .whitelist_function("pg.*")
            // Whitelist used functions
            .whitelist_function("longjmp")
            .whitelist_function("_setjmp")
            .whitelist_function("cstring_to_text")
            .whitelist_function("text_to_cstring")
            .whitelist_function("errmsg")
            .whitelist_function("errstart")
            .whitelist_function("errfinish")
            .whitelist_function("pfree")
            .whitelist_function("list_.*")
            .whitelist_function("palloc")
            .whitelist_function(".*array.*")
            .whitelist_function("get_typlenbyvalalign")
            // Whitelist all PG-related types
            .whitelist_type("PG.*")
            // Whitelist used types
            .whitelist_type("jmp_buf")
            .whitelist_type("text")
            .whitelist_type("varattrib_1b")
            .whitelist_type("varattrib_4b")
            .whitelist_type(".*Array.*")
            // Whitelist PG-related values
            .whitelist_var("PG.*")
            // Whitelist log-level values
            .whitelist_var("DEBUG.*")
            .whitelist_var("LOG.*")
            .whitelist_var("INFO")
            .whitelist_var("NOTICE")
            .whitelist_var("WARNING")
            .whitelist_var("ERROR")
            .whitelist_var("FATAL")
            .whitelist_var("PANIC")
            // Whitelist misc values
            .whitelist_var("CurrentMemoryContext")
            .whitelist_var("FUNC_MAX_ARGS")
            .whitelist_var("INDEX_MAX_KEYS")
            .whitelist_var("NAMEDATALEN")
            .whitelist_var("USE_FLOAT.*")
            // FDW whitelisting
            .whitelist_function("pstrdup")
            .whitelist_function("lappend")
            .whitelist_function("makeTargetEntry")
            .whitelist_function("makeVar")
            .whitelist_function("ExecStoreTuple")
            .whitelist_function("heap_form_tuple")
            .whitelist_function("ExecClearTuple")
            .whitelist_function("slot_getallattrs")
            .whitelist_function("get_rel_name")
            .whitelist_function("GetForeignTable")
            .whitelist_function("GetForeignServer")
            .whitelist_function("make_foreignscan")
            .whitelist_function("extract_actual_clauses")
            .whitelist_function("add_path")
            .whitelist_function("create_foreignscan_path")
            .whitelist_type("ImportForeignSchemaStmt")
            .whitelist_type("ResultRelInfo")
            .whitelist_type("EState")
            .whitelist_type("ModifyTableState")
            .whitelist_type("Relation")
            .whitelist_type("RangeTblEntry")
            .whitelist_type("Query")
            .whitelist_type("ForeignScanState")
            .whitelist_type("InvalidBuffer")
            .whitelist_type("RelationData")
            .whitelist_type("ForeignScan")
            .whitelist_type("Plan")
            .whitelist_type("ForeignPath")
            .whitelist_type("RelOptInfo")
            .whitelist_type("Form_pg_attribute")
            .whitelist_type("DefElem")
            .whitelist_type("Value")
            .whitelist_var("InvalidBuffer")
    }

    #[cfg(unix)]
    fn get_bindings(pg_include: &str) -> bindgen::Builder {
        bindgen::Builder::default().clang_arg(format!("-I{}", pg_include))
    }

    fn include_dir(pg_config: &str) -> Result<String, env::VarError> {
        env::var("PG_INCLUDE_PATH").or_else(|err| {
            match Command::new(pg_config).arg("--includedir-server").output() {
                Ok(out) => Ok(String::from_utf8(out.stdout).unwrap().trim().to_string()),
                Err(..) => Err(err),
            }
        })
    }

    #[derive(Debug)]
    struct IgnoreMacros(HashSet<String>);

    impl bindgen::callbacks::ParseCallbacks for IgnoreMacros {
        fn will_parse_macro(&self, name: &str) -> bindgen::callbacks::MacroParsingBehavior {
            if self.0.contains(name) {
                bindgen::callbacks::MacroParsingBehavior::Ignore
            } else {
                bindgen::callbacks::MacroParsingBehavior::Default
            }
        }
    }
}
