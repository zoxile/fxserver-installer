use crate::{
    models::fxserver::FxserverStatus,
    terminal::{safe_text, settings::Settings},
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Sparkline, Wrap},
    Frame,
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const RANGES: [(u64, &str); 5] = [
    (30, "30s"),
    (300, "5m"),
    (600, "10m"),
    (1800, "30m"),
    (3600, "1h"),
];

#[derive(Default)]
pub(super) struct History {
    samples: VecDeque<(Instant, u64, u64)>,
    pid: Option<u32>,
    started: Option<Instant>,
    extrema: Option<(u64, u64, u64, u64)>,
    pub(super) range: usize,
}

impl History {
    pub(super) fn sample(&mut self, status: &FxserverStatus) {
        if status.running && self.pid != status.pid {
            self.samples.clear();
            self.extrema = None;
            self.pid = status.pid;
            self.started = Some(Instant::now());
        } else if !status.running {
            self.pid = None;
            self.started = None;
            return;
        }
        if let Some(usage) = &status.resources {
            let cpu = (usage.cpu_percent.max(0.) * 10.) as u64;
            let ram = usage.memory_bytes / 1048576;
            self.samples.push_back((Instant::now(), cpu, ram));
            while self.samples.len() > 3600 {
                self.samples.pop_front();
            }
            self.extrema = Some(match self.extrema {
                Some((cl, ch, rl, rh)) => (cl.min(cpu), ch.max(cpu), rl.min(ram), rh.max(ram)),
                None => (cpu, cpu, ram, ram),
            });
        }
    }

    pub(super) fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        status: Option<&FxserverStatus>,
        settings: &Settings,
    ) {
        let uptime = self
            .started
            .map(|time| time.elapsed().as_secs())
            .unwrap_or(0);
        let mut text = format!(
            "Artifact: {}\ntxData: {}\nUptime {:02}:{:02}:{:02}",
            safe_text(&settings.artifact_path),
            safe_text(&settings.tx_data_path),
            uptime / 3600,
            (uptime / 60) % 60,
            uptime % 60
        );
        if let Some(status) = status {
            if let Some(pid) = status.pid {
                text.push_str(&format!("   PID {pid}"));
            }
            if let Some(usage) = &status.resources {
                text.push_str(&format!(
                    "\nCPU {:.1}%   RAM {} MiB\nThreads {}   Handles/FDs {}",
                    usage.cpu_percent,
                    usage.memory_bytes / 1048576,
                    usage.thread_count,
                    usage.handle_count
                ));
            }
        }
        if area.height < 13 {
            frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
            return;
        }
        let [details, cpu_area, ram_area] = Layout::vertical([
            Constraint::Length(5),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(text), details);
        let (seconds, label) = RANGES[self.range];
        let cutoff = Instant::now() - Duration::from_secs(seconds);
        let samples: Vec<_> = self
            .samples
            .iter()
            .filter(|(time, _, _)| *time >= cutoff)
            .collect();
        let bucket = samples.len().div_ceil(area.width.max(1) as usize).max(1);
        let cpu: Vec<_> = samples
            .chunks(bucket)
            .map(|chunk| chunk.iter().map(|(_, cpu, _)| *cpu).max().unwrap_or(0))
            .collect();
        let ram: Vec<_> = samples
            .chunks(bucket)
            .map(|chunk| chunk.iter().map(|(_, _, ram)| *ram).max().unwrap_or(0))
            .collect();
        let (cl, ch, rl, rh) = self.extrema.unwrap_or_default();
        let cpu_title = format!(
            "CPU {label} | session {:.1}-{:.1}%",
            cl as f64 / 10.,
            ch as f64 / 10.
        );
        let ram_title = format!("RAM {label} | session {rl}-{rh} MiB");
        frame.render_widget(
            Sparkline::default()
                .data(cpu)
                .max(1000)
                .style(Style::new().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL).title(cpu_title)),
            cpu_area,
        );
        frame.render_widget(
            Sparkline::default()
                .data(ram)
                .style(Style::new().fg(Color::Magenta))
                .block(Block::default().borders(Borders::ALL).title(ram_title)),
            ram_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fxserver::FxserverResources;
    #[test]
    fn history_is_bounded_and_new_sessions_reset_extrema() {
        let mut history = History::default();
        let mut status = FxserverStatus {
            running: true,
            pid: Some(1),
            artifact_path: None,
            started_at: None,
            uptime_seconds: None,
            resources: Some(FxserverResources {
                cpu_percent: 5.,
                memory_bytes: 1048576,
                total_memory_bytes: 0,
                memory_percent: 0.,
                thread_count: 0,
                handle_count: 0,
            }),
        };
        for _ in 0..4000 {
            history.sample(&status);
        }
        assert_eq!(history.samples.len(), 3600);
        status.pid = Some(2);
        history.sample(&status);
        assert_eq!(history.samples.len(), 1);
        status.running = false;
        history.sample(&status);
        assert!(history.started.is_none());
    }
}
