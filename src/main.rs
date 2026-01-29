use crossterm::{
    cursor::{self, MoveTo},
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    style::{Color, ResetColor, SetForegroundColor, Print},
    terminal::{self, Clear, ClearType},
};
use regex::Regex;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{self, Write};
use std::process::Command;
use std::time::Duration;

const HIST_W: usize = 42; 
const HIST_H: usize = 4;  

#[derive(Clone, Copy)]
struct MemPoint { val1: u64, val2: u64, total: u64 }

struct AppState {
    version: String, interval_ms: u64, log_lines: u32, color_mode: u8,
    peak_cpu: u64, peak_temp: f64,
    prev_idle: u64, prev_total: u64,
    prev_net_tx: u64, prev_net_errs: u64,
    cpu_history: VecDeque<u64>,
    temp_history: VecDeque<u64>,
    ram_history: VecDeque<MemPoint>,
    cma_history: VecDeque<MemPoint>,
}

fn run_cmd(cmd: &str, args: &[&str]) -> String {
    let output = Command::new(cmd).args(args).output();
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

fn draw_stacked_graph(stdout: &mut io::Stdout, x: u16, y: u16, history: &VecDeque<MemPoint>, color1: Color, color2: Color) -> io::Result<()> {
    for row in 0..HIST_H {
        execute!(stdout, MoveTo(x, y + row as u16))?;
        for pt in history {
            let max = pt.total.max(1);
            let threshold = ((HIST_H - row) as u64 * max) / HIST_H as u64;
            if pt.val1 >= threshold { execute!(stdout, SetForegroundColor(color1), Print("█"))?; }
            else if pt.val2 >= threshold { execute!(stdout, SetForegroundColor(color2), Print("▓"))?; }
            else { execute!(stdout, SetForegroundColor(Color::Rgb { r: 60, g: 60, b: 60 }), Print("░"))?; }
        }
    }
    Ok(())
}

fn draw_simple_graph(stdout: &mut io::Stdout, x: u16, y: u16, history: &VecDeque<u64>, max_val: u64, color: Color) -> io::Result<()> {
    for row in 0..HIST_H {
        execute!(stdout, MoveTo(x, y + row as u16), SetForegroundColor(color))?;
        for &val in history {
            let threshold = ((HIST_H - row) as u64 * max_val) / HIST_H as u64;
            if val >= threshold { execute!(stdout, Print("█"))?; }
            else { execute!(stdout, Print(" "))?; }
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide, Clear(ClearType::All))?;

    let mut state = AppState {
        version: "23.0-rs".to_string(), interval_ms: 1000, log_lines: 10, color_mode: 1,
        peak_cpu: 0, peak_temp: 0.0, prev_idle: 0, prev_total: 0, prev_net_tx: 0, prev_net_errs: 0,
        cpu_history: VecDeque::from(vec![0; HIST_W]),
        temp_history: VecDeque::from(vec![0; HIST_W]),
        ram_history: VecDeque::from(vec![MemPoint { val1: 0, val2: 0, total: 100 }; HIST_W]),
        cma_history: VecDeque::from(vec![MemPoint { val1: 0, val2: 0, total: 100 }; HIST_W]),
    };

    let re_ws = Regex::new(r"\s+").unwrap();

    loop {
        let (cols, rows) = terminal::size().unwrap_or((120, 30));
        let mid_x = (cols / 2).max(55);

        // --- DATA ---
        let stat_s = fs::read_to_string("/proc/stat").unwrap_or_default();
        let load_s = fs::read_to_string("/proc/loadavg").unwrap_or_default();
        let cpu_line = stat_s.lines().next().unwrap_or("");
        let cpu_p: Vec<&str> = re_ws.split(cpu_line.trim()).collect();
        let mut cpu_usage = 0;
        if cpu_p.len() > 4 {
            let total: u64 = cpu_p[1..8].iter().map(|s| s.parse().unwrap_or(0)).sum();
            let idle: u64 = cpu_p[4].parse().unwrap_or(0);
            let d_tot = total.saturating_sub(state.prev_total);
            let d_idl = idle.saturating_sub(state.prev_idle);
            if d_tot > 0 { cpu_usage = 100 * (d_tot - d_idl) / d_tot; }
            state.prev_total = total; state.prev_idle = idle;
            if cpu_usage > state.peak_cpu { state.peak_cpu = cpu_usage; }
        }
        state.cpu_history.push_back(cpu_usage); state.cpu_history.pop_front();

        let temp_f: f64 = run_cmd("vcgencmd", &["measure_temp"]).replace("temp=", "").replace("'C", "").parse().unwrap_or(0.0);
        if temp_f > state.peak_temp { state.peak_temp = temp_f; }
        state.temp_history.push_back(temp_f as u64); state.temp_history.pop_front();

        let mem_s = fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let mut m = HashMap::new();
        for l in mem_s.lines() {
            let p: Vec<&str> = l.split_whitespace().collect();
            if p.len() >= 2 { m.insert(p[0].replace(":", ""), p[1].parse::<u64>().unwrap_or(0)); }
        }
        let m_tot = *m.get("MemTotal").unwrap_or(&1);
        let m_cach = *m.get("Cached").unwrap_or(&0) + *m.get("Buffers").unwrap_or(&0);
        let ram_app = m_tot.saturating_sub(*m.get("MemFree").unwrap_or(&0)).saturating_sub(m_cach);
        state.ram_history.push_back(MemPoint { val1: ram_app, val2: ram_app + m_cach, total: m_tot });
        state.ram_history.pop_front();

        let cma_tot = *m.get("CmaTotal").unwrap_or(&1);
        let cma_res = cma_tot.saturating_sub(*m.get("CmaFree").unwrap_or(&0));
        let dma_out = run_cmd("sudo", &["cat", "/sys/kernel/debug/dma_buf/bufinfo"]);
        let mut dma_active = 0u64;
        let mut dma_cats: HashMap<String, (u64, u64)> = HashMap::new();
        for l in dma_out.lines() {
            let p: Vec<&str> = l.split_whitespace().collect();
            if l.contains("Total") && p.len() >= 4 { dma_active = p[3].parse().unwrap_or(0); }
            if p.len() >= 6 && !l.starts_with("size") && !l.contains("Total") {
                if let Ok(sz) = u64::from_str_radix(p[0], 16) {
                    let n = if l.contains("vc_sm") { "GPU Shared" } else if l.contains("rpicam") { "Camera App" } else { "Sys/ISP" };
                    let e = dma_cats.entry(n.to_string()).or_insert((0, 0)); e.0 += sz; e.1 += 1;
                }
            }
        }
        state.cma_history.push_back(MemPoint { val1: dma_active/1024, val2: cma_res, total: cma_tot });
        state.cma_history.pop_front();

        let net_s = fs::read_to_string("/proc/net/dev").unwrap_or_default();
        let mut tx_bytes = 0u64; let mut net_errs = 0u64;
        if let Some(l) = net_s.lines().find(|l| l.contains("wlan0") || l.contains("eth0")) {
            let p: Vec<&str> = l.split_whitespace().collect();
            if p.len() > 10 {
                tx_bytes = p[9].parse().unwrap_or(0);
                net_errs = p[2].parse::<u64>().unwrap_or(0) + p[3].parse::<u64>().unwrap_or(0);
            }
        }
        let net_speed = (tx_bytes.saturating_sub(state.prev_net_tx)) * 8 / 1024;
        let err_diff = net_errs.saturating_sub(state.prev_net_errs);
        state.prev_net_tx = tx_bytes; state.prev_net_errs = net_errs;

        // --- DRAW ---
        execute!(stdout, MoveTo(0, 0), SetForegroundColor(Color::Cyan), Print(format!("=== PI DASHBOARD === {} (v{})", chrono::Local::now().format("%H:%M:%S"), state.version)))?;
        execute!(stdout, MoveTo(0, 1), SetForegroundColor(Color::White), Print(format!("Uptime: {} | Load: {}", run_cmd("uptime", &["-p"]).replace("up ", ""), load_s.trim())))?;
        execute!(stdout, MoveTo(0, 2), SetForegroundColor(Color::DarkGrey), Print("-".repeat(cols as usize)))?;

        // Graphs
        execute!(stdout, MoveTo(0, 3), SetForegroundColor(Color::Yellow), Print("CPU HISTORY"), MoveTo(mid_x, 3), Print("TEMP HISTORY"))?;
        draw_simple_graph(&mut stdout, 0, 4, &state.cpu_history, 100, Color::Green)?;
        draw_simple_graph(&mut stdout, mid_x, 4, &state.temp_history, 80, Color::Red)?;
        execute!(stdout, MoveTo(0, 8), ResetColor, Print(format!("Usage: {}% (Peak: {}%)", cpu_usage, state.peak_cpu)), MoveTo(mid_x, 8), Print(format!("Temp: {:.1}°C (Peak: {}°C)", temp_f, state.peak_temp)))?;

        execute!(stdout, MoveTo(0, 10), SetForegroundColor(Color::Yellow), Print("RAM (App/Cache/Tot)"), MoveTo(mid_x, 10), Print("CMA (Act/Res/Tot)"))?;
        draw_stacked_graph(&mut stdout, 0, 11, &state.ram_history, Color::Blue, Color::Cyan)?;
        draw_stacked_graph(&mut stdout, mid_x, 11, &state.cma_history, Color::Magenta, Color::DarkMagenta)?;
        execute!(stdout, MoveTo(0, 15), ResetColor, Print(format!("{}/{}/{} MB", ram_app/1024, m_cach/1024, m_tot/1024)), MoveTo(mid_x, 15), Print(format!("{}/{}/{} MB", dma_active/1024/1024, cma_res/1024, cma_tot/1024)))?;

        // Hardware Details
        execute!(stdout, MoveTo(0, 17), SetForegroundColor(Color::Yellow), Print("[HARDWARE & HEALTH]"), MoveTo(mid_x, 17), Print("[DMA-BUFFER DETAILS]"))?;
        let volt = run_cmd("vcgencmd", &["measure_volts", "core"]).replace("volt=", "");
        let h264 = run_cmd("vcgencmd", &["measure_clock", "h264"]).split('=').last().unwrap_or("0").parse::<u64>().unwrap_or(0)/1000000;
        execute!(stdout, MoveTo(0, 18), ResetColor, Print(format!("Volt: {:<7} | H264: {}MHz", volt, h264)))?;
        execute!(stdout, MoveTo(0, 19), Print(format!("Net Errs/s: {:<3} | Throttled: ", err_diff)))?;
        let thr = run_cmd("vcgencmd", &["get_throttled"]).replace("throttled=", "");
        if thr == "0x0" { execute!(stdout, SetForegroundColor(Color::Green), Print("None"))?; } else { execute!(stdout, SetForegroundColor(Color::Red), Print(&thr))?; }

        let mut dma_v: Vec<_> = dma_cats.iter().collect(); dma_v.sort_by(|a, b| b.1.0.cmp(&a.1.0));
        for (i, (n, (sz, ct))) in dma_v.iter().take(3).enumerate() {
            execute!(stdout, MoveTo(mid_x, 18 + i as u16), ResetColor, Print(format!("{:<12} | {:>7.1}MB | #{}", n, *sz as f64 / 1024.0 / 1024.0, ct)))?;
        }

        execute!(stdout, MoveTo(0, 21), SetForegroundColor(Color::DarkGrey), Print("-".repeat(cols as usize)))?;
        execute!(stdout, MoveTo(0, 22), SetForegroundColor(Color::Cyan), Print(format!("Net: {:>5} kbps Out", net_speed)), ResetColor, Print(" | "), SetForegroundColor(Color::Cyan), Print(format!("Stream: {}", run_cmd("ps", &["-C", "rpicam-vid,ffmpeg", "-o", "comm="]))))?;
        execute!(stdout, MoveTo(0, 23), SetForegroundColor(Color::DarkGrey), Print("-".repeat(cols as usize)))?;

        // Logs
        let log_y = 24;
        execute!(stdout, MoveTo(0, log_y), SetForegroundColor(Color::Yellow), Print(format!("[SYSTEMD LOG: stream.service ({} lines)]", state.log_lines)))?;
        let logs = run_cmd("journalctl", &["-u", "stream.service", "-n", &state.log_lines.to_string(), "--no-pager"]);
        for (i, l) in logs.lines().enumerate() {
            if log_y + 1 + i as u16 >= rows - 1 { break; }
            execute!(stdout, MoveTo(0, log_y + 1 + i as u16), ResetColor, Print(if l.len() > cols as usize { &l[l.len()-cols as usize..] } else { l }), Clear(ClearType::UntilNewLine))?;
        }

        // Footer
        let speed_label = if state.interval_ms < 1000 { format!("{}ms", state.interval_ms) } else { format!("{}s", state.interval_ms/1000) };
        execute!(stdout, MoveTo(0, rows-1), SetForegroundColor(Color::DarkGrey), Print(format!("Controls: [+/-] Speed ({}) | [c] Color | [,/.] Logs ({}) | [q] Exit", speed_label, state.log_lines)), Clear(ClearType::UntilNewLine))?;
        
        stdout.flush()?;
        if event::poll(Duration::from_millis(state.interval_ms))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('+') => state.interval_ms = (state.interval_ms.saturating_sub(100)).max(100),
                        KeyCode::Char('-') => state.interval_ms = (state.interval_ms + 100).min(5000),
                        KeyCode::Char('.') => { state.log_lines += 1; execute!(stdout, Clear(ClearType::All))?; },
                        KeyCode::Char(',') => { if state.log_lines > 1 { state.log_lines -= 1; execute!(stdout, Clear(ClearType::All))?; } },
                        _ => {}
                    }
                }
            }
        }
    }
    execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    Ok(())
}
