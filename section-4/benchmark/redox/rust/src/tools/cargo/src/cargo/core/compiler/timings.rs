//! Timing tracking.
//!
//! This module implements some simple tracking information for timing of how
//! long it takes for different units to compile.
use super::{CompileMode, Unit};
use crate::core::compiler::BuildContext;
use crate::core::PackageId;
use crate::util::cpu::State;
use crate::util::machine_message::{self, Message};
use crate::util::{paths, CargoResult, Config};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant, SystemTime};

pub struct Timings<'a, 'cfg> {
    config: &'cfg Config,
    /// Whether or not timings should be captured.
    enabled: bool,
    /// If true, saves an HTML report to disk.
    report_html: bool,
    /// If true, reports unit completion to stderr.
    report_info: bool,
    /// If true, emits JSON information with timing information.
    report_json: bool,
    /// When Cargo started.
    start: Instant,
    /// A rendered string of when compilation started.
    start_str: String,
    /// A summary of the root units.
    ///
    /// Tuples of `(package_description, target_descrptions)`.
    root_targets: Vec<(String, Vec<String>)>,
    /// The build profile.
    profile: String,
    /// Total number of fresh units.
    total_fresh: u32,
    /// Total number of dirty units.
    total_dirty: u32,
    /// Time tracking for each individual unit.
    unit_times: Vec<UnitTime<'a>>,
    /// Units that are in the process of being built.
    /// When they finished, they are moved to `unit_times`.
    active: HashMap<u32, UnitTime<'a>>,
    /// Concurrency-tracking information. This is periodically updated while
    /// compilation progresses.
    concurrency: Vec<Concurrency>,
    /// Last recorded state of the system's CPUs and when it happened
    last_cpu_state: Option<State>,
    last_cpu_recording: Instant,
    /// Recorded CPU states, stored as tuples. First element is when the
    /// recording was taken and second element is percentage usage of the
    /// system.
    cpu_usage: Vec<(f64, f64)>,
}

/// Tracking information for an individual unit.
struct UnitTime<'a> {
    unit: Unit<'a>,
    /// A string describing the cargo target.
    target: String,
    /// The time when this unit started as an offset in seconds from `Timings::start`.
    start: f64,
    /// Total time to build this unit in seconds.
    duration: f64,
    /// The time when the `.rmeta` file was generated, an offset in seconds
    /// from `start`.
    rmeta_time: Option<f64>,
    /// Reverse deps that are freed to run after this unit finished.
    unlocked_units: Vec<Unit<'a>>,
    /// Same as `unlocked_units`, but unlocked by rmeta.
    unlocked_rmeta_units: Vec<Unit<'a>>,
}

/// Periodic concurrency tracking information.
#[derive(serde::Serialize)]
struct Concurrency {
    /// Time as an offset in seconds from `Timings::start`.
    t: f64,
    /// Number of units currently running.
    active: usize,
    /// Number of units that could run, but are waiting for a jobserver token.
    waiting: usize,
    /// Number of units that are not yet ready, because they are waiting for
    /// dependencies to finish.
    inactive: usize,
}

impl<'a, 'cfg> Timings<'a, 'cfg> {
    pub fn new(bcx: &BuildContext<'a, 'cfg>, root_units: &[Unit<'_>]) -> Timings<'a, 'cfg> {
        let has_report = |what| {
            bcx.config
                .cli_unstable()
                .timings
                .as_ref()
                .map_or(false, |t| t.iter().any(|opt| opt == what))
        };
        let report_html = has_report("html");
        let report_info = has_report("info");
        let report_json = has_report("json");
        let enabled = report_html | report_info | report_json;

        let mut root_map: HashMap<PackageId, Vec<String>> = HashMap::new();
        for unit in root_units {
            let target_desc = unit.target.description_named();
            root_map
                .entry(unit.pkg.package_id())
                .or_default()
                .push(target_desc);
        }
        let root_targets = root_map
            .into_iter()
            .map(|(pkg_id, targets)| {
                let pkg_desc = format!("{} {}", pkg_id.name(), pkg_id.version());
                (pkg_desc, targets)
            })
            .collect();
        let start_str = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
        let profile = bcx.build_config.profile_kind.name().to_owned();

        Timings {
            config: bcx.config,
            enabled,
            report_html,
            report_info,
            report_json,
            start: bcx.config.creation_time(),
            start_str,
            root_targets,
            profile,
            total_fresh: 0,
            total_dirty: 0,
            unit_times: Vec::new(),
            active: HashMap::new(),
            concurrency: Vec::new(),
            last_cpu_state: if enabled { State::current().ok() } else { None },
            last_cpu_recording: Instant::now(),
            cpu_usage: Vec::new(),
        }
    }

    /// Mark that a unit has started running.
    pub fn unit_start(&mut self, id: u32, unit: Unit<'a>) {
        if !self.enabled {
            return;
        }
        let mut target = if unit.target.is_lib() && unit.mode == CompileMode::Build {
            // Special case for brevity, since most dependencies hit
            // this path.
            "".to_string()
        } else {
            format!(" {}", unit.target.description_named())
        };
        match unit.mode {
            CompileMode::Test => target.push_str(" (test)"),
            CompileMode::Build => {}
            CompileMode::Check { test: true } => target.push_str(" (check-test)"),
            CompileMode::Check { test: false } => target.push_str(" (check)"),
            CompileMode::Bench => target.push_str(" (bench)"),
            CompileMode::Doc { .. } => target.push_str(" (doc)"),
            CompileMode::Doctest => target.push_str(" (doc test)"),
            CompileMode::RunCustomBuild => target.push_str(" (run)"),
        }
        let unit_time = UnitTime {
            unit,
            target,
            start: d_as_f64(self.start.elapsed()),
            duration: 0.0,
            rmeta_time: None,
            unlocked_units: Vec::new(),
            unlocked_rmeta_units: Vec::new(),
        };
        assert!(self.active.insert(id, unit_time).is_none());
    }

    /// Mark that the `.rmeta` file as generated.
    pub fn unit_rmeta_finished(&mut self, id: u32, unlocked: Vec<&Unit<'a>>) {
        if !self.enabled {
            return;
        }
        // `id` may not always be active. "fresh" units unconditionally
        // generate `Message::Finish`, but this active map only tracks dirty
        // units.
        let unit_time = match self.active.get_mut(&id) {
            Some(ut) => ut,
            None => return,
        };
        let t = d_as_f64(self.start.elapsed());
        unit_time.rmeta_time = Some(t - unit_time.start);
        assert!(unit_time.unlocked_rmeta_units.is_empty());
        unit_time.unlocked_rmeta_units.extend(unlocked);
    }

    /// Mark that a unit has finished running.
    pub fn unit_finished(&mut self, id: u32, unlocked: Vec<&Unit<'a>>) {
        if !self.enabled {
            return;
        }
        // See note above in `unit_rmeta_finished`, this may not always be active.
        let mut unit_time = match self.active.remove(&id) {
            Some(ut) => ut,
            None => return,
        };
        let t = d_as_f64(self.start.elapsed());
        unit_time.duration = t - unit_time.start;
        assert!(unit_time.unlocked_units.is_empty());
        unit_time.unlocked_units.extend(unlocked);
        if self.report_info {
            let msg = format!(
                "{}{} in {:.1}s",
                unit_time.name_ver(),
                unit_time.target,
                unit_time.duration
            );
            let _ = self
                .config
                .shell()
                .status_with_color("Completed", msg, termcolor::Color::Cyan);
        }
        if self.report_json {
            let msg = machine_message::TimingInfo {
                package_id: unit_time.unit.pkg.package_id(),
                target: unit_time.unit.target,
                mode: unit_time.unit.mode,
                duration: unit_time.duration,
                rmeta_time: unit_time.rmeta_time,
            }
            .to_json_string();
            self.config.shell().stdout_println(msg);
        }
        self.unit_times.push(unit_time);
    }

    /// This is called periodically to mark the concurrency of internal structures.
    pub fn mark_concurrency(&mut self, active: usize, waiting: usize, inactive: usize) {
        if !self.enabled {
            return;
        }
        let c = Concurrency {
            t: d_as_f64(self.start.elapsed()),
            active,
            waiting,
            inactive,
        };
        self.concurrency.push(c);
    }

    /// Mark that a fresh unit was encountered.
    pub fn add_fresh(&mut self) {
        self.total_fresh += 1;
    }

    /// Mark that a dirty unit was encountered.
    pub fn add_dirty(&mut self) {
        self.total_dirty += 1;
    }

    /// Take a sample of CPU usage
    pub fn record_cpu(&mut self) {
        if !self.enabled {
            return;
        }
        let prev = match &mut self.last_cpu_state {
            Some(state) => state,
            None => return,
        };
        // Don't take samples too too frequently, even if requested.
        let now = Instant::now();
        if self.last_cpu_recording.elapsed() < Duration::from_millis(100) {
            return;
        }
        let current = match State::current() {
            Ok(s) => s,
            Err(_) => return,
        };
        let pct_idle = current.idle_since(prev);
        *prev = current;
        self.last_cpu_recording = now;
        let dur = d_as_f64(now.duration_since(self.start));
        self.cpu_usage.push((dur, 100.0 - pct_idle));
    }

    /// Call this when all units are finished.
    pub fn finished(&mut self, bcx: &BuildContext<'_, '_>) -> CargoResult<()> {
        if !self.enabled {
            return Ok(());
        }
        self.mark_concurrency(0, 0, 0);
        self.unit_times
            .sort_unstable_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        if self.report_html {
            self.report_html(bcx)?;
        }
        Ok(())
    }

    /// Save HTML report to disk.
    fn report_html(&self, bcx: &BuildContext<'_, '_>) -> CargoResult<()> {
        let duration = d_as_f64(self.start.elapsed());
        let timestamp = self.start_str.replace(&['-', ':'][..], "");
        let filename = format!("cargo-timing-{}.html", timestamp);
        let mut f = BufWriter::new(File::create(&filename)?);
        let roots: Vec<&str> = self
            .root_targets
            .iter()
            .map(|(name, _targets)| name.as_str())
            .collect();
        f.write_all(HTML_TMPL.replace("{ROOTS}", &roots.join(", ")).as_bytes())?;
        self.write_summary_table(&mut f, duration, bcx)?;
        f.write_all(HTML_CANVAS.as_bytes())?;
        self.write_unit_table(&mut f)?;
        // It helps with pixel alignment to use whole numbers.
        writeln!(
            f,
            "<script>\n\
             DURATION = {};",
            f64::ceil(duration) as u32
        )?;
        self.write_js_data(&mut f)?;
        write!(
            f,
            "{}\n\
             </script>\n\
             </body>\n\
             </html>\n\
             ",
            include_str!("timings.js")
        )?;
        drop(f);
        let msg = format!(
            "report saved to {}",
            std::env::current_dir()
                .unwrap_or_default()
                .join(&filename)
                .display()
        );
        paths::link_or_copy(&filename, "cargo-timing.html")?;
        self.config
            .shell()
            .status_with_color("Timing", msg, termcolor::Color::Cyan)?;
        Ok(())
    }

    /// Render the summary table.
    fn write_summary_table(
        &self,
        f: &mut impl Write,
        duration: f64,
        bcx: &BuildContext<'_, '_>,
    ) -> CargoResult<()> {
        let targets: Vec<String> = self
            .root_targets
            .iter()
            .map(|(name, targets)| format!("{} ({})", name, targets.join(", ")))
            .collect();
        let targets = targets.join("<br>");
        let time_human = if duration > 60.0 {
            format!(" ({}m {:.1}s)", duration as u32 / 60, duration % 60.0)
        } else {
            "".to_string()
        };
        let total_time = format!("{:.1}s{}", duration, time_human);
        let max_concurrency = self.concurrency.iter().map(|c| c.active).max().unwrap();
        let rustc_info = render_rustc_info(bcx);
        write!(
            f,
            r#"
<table class="my-table summary-table">
  <tr>
    <td>Targets:</td><td>{}</td>
  </tr>
  <tr>
    <td>Profile:</td><td>{}</td>
  </tr>
  <tr>
    <td>Fresh units:</td><td>{}</td>
  </tr>
  <tr>
    <td>Dirty units:</td><td>{}</td>
  </tr>
  <tr>
    <td>Total units:</td><td>{}</td>
  </tr>
  <tr>
    <td>Max concurrency:</td><td>{} (jobs={} ncpu={})</td>
  </tr>
  <tr>
    <td>Build start:</td><td>{}</td>
  </tr>
  <tr>
    <td>Total time:</td><td>{}</td>
  </tr>
  <tr>
    <td>rustc:</td><td>{}</td>
  </tr>

</table>
"#,
            targets,
            self.profile,
            self.total_fresh,
            self.total_dirty,
            self.total_fresh + self.total_dirty,
            max_concurrency,
            bcx.build_config.jobs,
            num_cpus::get(),
            self.start_str,
            total_time,
            rustc_info,
        )?;
        Ok(())
    }

    fn write_js_data(&self, f: &mut impl Write) -> CargoResult<()> {
        // Create a map to link indices of unlocked units.
        let unit_map: HashMap<Unit<'_>, usize> = self
            .unit_times
            .iter()
            .enumerate()
            .map(|(i, ut)| (ut.unit, i))
            .collect();
        #[derive(serde::Serialize)]
        struct UnitData {
            i: usize,
            name: String,
            version: String,
            mode: String,
            target: String,
            start: f64,
            duration: f64,
            rmeta_time: Option<f64>,
            unlocked_units: Vec<usize>,
            unlocked_rmeta_units: Vec<usize>,
        }
        let round = |x: f64| (x * 100.0).round() / 100.0;
        let unit_data: Vec<UnitData> = self
            .unit_times
            .iter()
            .enumerate()
            .map(|(i, ut)| {
                let mode = if ut.unit.mode.is_run_custom_build() {
                    "run-custom-build"
                } else {
                    "todo"
                }
                .to_string();

                // These filter on the unlocked units because not all unlocked
                // units are actually "built". For example, Doctest mode units
                // don't actually generate artifacts.
                let unlocked_units: Vec<usize> = ut
                    .unlocked_units
                    .iter()
                    .filter_map(|unit| unit_map.get(unit).copied())
                    .collect();
                let unlocked_rmeta_units: Vec<usize> = ut
                    .unlocked_rmeta_units
                    .iter()
                    .filter_map(|unit| unit_map.get(unit).copied())
                    .collect();
                UnitData {
                    i,
                    name: ut.unit.pkg.name().to_string(),
                    version: ut.unit.pkg.version().to_string(),
                    mode,
                    target: ut.target.clone(),
                    start: round(ut.start),
                    duration: round(ut.duration),
                    rmeta_time: ut.rmeta_time.map(|t| round(t)),
                    unlocked_units,
                    unlocked_rmeta_units,
                }
            })
            .collect();
        writeln!(
            f,
            "const UNIT_DATA = {};",
            serde_json::to_string_pretty(&unit_data)?
        )?;
        writeln!(
            f,
            "const CONCURRENCY_DATA = {};",
            serde_json::to_string_pretty(&self.concurrency)?
        )?;
        writeln!(
            f,
            "const CPU_USAGE = {};",
            serde_json::to_string_pretty(&self.cpu_usage)?
        )?;
        Ok(())
    }

    /// Render the table of all units.
    fn write_unit_table(&self, f: &mut impl Write) -> CargoResult<()> {
        write!(
            f,
            r#"
<table class="my-table">
  <thead>
    <tr>
      <th></th>
      <th>Unit</th>
      <th>Total</th>
      <th>Codegen</th>
      <th>Features</th>
    </tr>
  </thead>
  <tbody>
"#
        )?;
        let mut units: Vec<&UnitTime<'_>> = self.unit_times.iter().collect();
        units.sort_unstable_by(|a, b| b.duration.partial_cmp(&a.duration).unwrap());
        for (i, unit) in units.iter().enumerate() {
            let codegen = match unit.codegen_time() {
                None => "".to_string(),
                Some((_rt, ctime, cent)) => format!("{:.1}s ({:.0}%)", ctime, cent),
            };
            let features = unit.unit.features.join(", ");
            write!(
                f,
                r#"
<tr>
  <td>{}.</td>
  <td>{}{}</td>
  <td>{:.1}s</td>
  <td>{}</td>
  <td>{}</td>
</tr>
"#,
                i + 1,
                unit.name_ver(),
                unit.target,
                unit.duration,
                codegen,
                features,
            )?;
        }
        write!(f, "</tbody>\n</table>\n")?;
        Ok(())
    }
}

impl<'a> UnitTime<'a> {
    /// Returns the codegen time as (rmeta_time, codegen_time, percent of total)
    fn codegen_time(&self) -> Option<(f64, f64, f64)> {
        self.rmeta_time.map(|rmeta_time| {
            let ctime = self.duration - rmeta_time;
            let cent = (ctime / self.duration) * 100.0;
            (rmeta_time, ctime, cent)
        })
    }

    fn name_ver(&self) -> String {
        format!("{} v{}", self.unit.pkg.name(), self.unit.pkg.version())
    }
}

// Replace with as_secs_f64 when 1.38 hits stable.
fn d_as_f64(d: Duration) -> f64 {
    (d.as_secs() as f64) + f64::from(d.subsec_nanos()) / 1_000_000_000.0
}

fn render_rustc_info(bcx: &BuildContext<'_, '_>) -> String {
    let version = bcx
        .rustc
        .verbose_version
        .lines()
        .next()
        .expect("rustc version");
    let requested_target = bcx.build_config.requested_kind.short_name(bcx);
    format!(
        "{}<br>Host: {}<br>Target: {}",
        version, bcx.rustc.host, requested_target
    )
}

static HTML_TMPL: &str = r#"
<html>
<head>
  <title>Cargo Build Timings — {ROOTS}</title>
  <meta charset="utf-8">
<style type="text/css">
html {
  font-family: sans-serif;
}

.canvas-container {
  position: relative;
  margin-top: 5px;
  margin-bottom: 5px;
}

h1 {
  border-bottom: 1px solid #c0c0c0;
}

.graph {
  display: block;
}

.my-table {
  margin-top: 20px;
  margin-bottom: 20px;
  border-collapse: collapse;
  box-shadow: 0 5px 10px rgba(0, 0, 0, 0.1);
}

.my-table th {
  color: #d5dde5;
  background: #1b1e24;
  border-bottom: 4px solid #9ea7af;
  border-right: 1px solid #343a45;
  font-size: 18px;
  font-weight: 100;
  padding: 12px;
  text-align: left;
  vertical-align: middle;
}

.my-table th:first-child {
  border-top-left-radius: 3px;
}

.my-table th:last-child {
  border-top-right-radius: 3px;
  border-right:none;
}

.my-table tr {
  border-top: 1px solid #c1c3d1;
  border-bottom: 1px solid #c1c3d1;
  font-size: 16px;
  font-weight: normal;
}

.my-table tr:first-child {
  border-top:none;
}

.my-table tr:last-child {
  border-bottom:none;
}

.my-table tr:nth-child(odd) td {
  background: #ebebeb;
}

.my-table tr:last-child td:first-child {
  border-bottom-left-radius:3px;
}

.my-table tr:last-child td:last-child {
  border-bottom-right-radius:3px;
}

.my-table td {
  background: #ffffff;
  padding: 10px;
  text-align: left;
  vertical-align: middle;
  font-weight: 300;
  font-size: 14px;
  border-right: 1px solid #C1C3D1;
}

.my-table td:last-child {
  border-right: 0px;
}

.summary-table td:first-child {
  vertical-align: top;
  text-align: right;
}

.input-table td {
  text-align: center;
}

</style>
</head>
<body>

<h1>Cargo Build Timings</h1>
"#;

static HTML_CANVAS: &str = r#"
<table class="input-table">
  <tr>
    <td><label for="min-unit-time">Min unit time:</label></td>
    <td><label for="scale">Scale:</label></td>
  </tr>
  <tr>
    <td><input type="range" min="0" max="30" step="0.1" value="0" id="min-unit-time"></td>
    <td><input type="range" min="1" max="50" value="20" id="scale"></td>
  </tr>
  <tr>
    <td><output for="min-unit-time" id="min-unit-time-output"></output></td>
    <td><output for="scale" id="scale-output"></output></td>
  </tr>
</table>

<div id="pipeline-container" class="canvas-container">
 <canvas id="pipeline-graph" class="graph" style="position: absolute; left: 0; top: 0; z-index: 0;"></canvas>
 <canvas id="pipeline-graph-lines" style="position: absolute; left: 0; top: 0; z-index: 1; pointer-events:none;"></canvas>
</div>
<div class="canvas-container">
  <canvas id="timing-graph" class="graph"></canvas>
</div>
"#;
