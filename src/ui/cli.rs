use crate::app::App;
use std::error::Error;

/// CLI主循环，原本在app.rs
pub async fn run_cli(app: &mut App) -> Result<(), Box<dyn Error>> {
    app.logger.info("Starting CLI application loop.");
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, std::sync::atomic::Ordering::SeqCst);
        println!("\nCtrl-C received, initiating shutdown...");
    })?;

    let print_interval = std::time::Duration::from_secs(app.config.cli_update_interval_secs);
    let mut last_print_time = std::time::Instant::now();

    if app.config.start_paused {
        app.logger.info(
            "Application configured to start paused. Data generators will not start automatically.",
        );
    } else {
        app.spawn_data_generators();
    }

    while running.load(std::sync::atomic::Ordering::SeqCst) {
        if app.config.run_duration.as_secs() > 0
            && app.stats.lock().await.start_time.elapsed() >= app.config.run_duration
        {
            app.logger.info(&format!(
                "Configured run duration of {:?} reached. Stopping.",
                app.config.run_duration
            ));
            running.store(false, std::sync::atomic::Ordering::SeqCst);
            break;
        }

        let mut stats_guard = app.stats.lock().await;
        let _stats_updated = app.stats_updater.update_stats(
            &mut *stats_guard,
            &mut app.target_stats_rx,
            &app.logger,
        );
        drop(stats_guard);

        app.manage_data_generator().await;

        if last_print_time.elapsed() >= print_interval {
            let stats_guard = app.stats.lock().await;
            let stats = &*stats_guard;
            let remaining_time = if app.config.run_duration.as_secs() > 0 {
                format!(
                    "(remaining: {:?})",
                    app.config
                        .run_duration
                        .saturating_sub(stats.start_time.elapsed())
                )
            } else {
                String::new()
            };
            println!(
                "{} ----- Stats ----- {}",
                chrono::Utc::now().to_rfc3339(),
                remaining_time
            );
            println!(
                "Total: {}, Success: {}, Failure: {}, RPS: {}",
                stats.get_total(),
                stats.get_success(),
                stats.get_failure(),
                stats.rps_history.back().copied().unwrap_or(0u64)
            );
            for target_stat in &stats.targets {
                println!(
                    "  Target {}: Success: {}, Failure: {}",
                    target_stat.id, target_stat.success, target_stat.failure
                );
            }
            println!("--------------------");
            last_print_time = std::time::Instant::now();
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    // 结尾打印一次
    let stats_guard = app.stats.lock().await;
    let stats = &*stats_guard;
    let remaining_time = if app.config.run_duration.as_secs() > 0 {
        format!(
            "(remaining: {:?})",
            app.config
                .run_duration
                .saturating_sub(stats.start_time.elapsed())
        )
    } else {
        String::new()
    };
    println!(
        "{} ----- Stats ----- {}",
        chrono::Utc::now().to_rfc3339(),
        remaining_time
    );
    println!(
        "Total: {}, Success: {}, Failure: {}, RPS: {}",
        stats.get_total(),
        stats.get_success(),
        stats.get_failure(),
        stats.rps_history.back().copied().unwrap_or(0u64)
    );
    for target_stat in &stats.targets {
        println!(
            "  Target {}: Success: {}, Failure: {}",
            target_stat.id, target_stat.success, target_stat.failure
        );
    }
    println!("--------------------");
    Ok(())
}
