use clap::Parser;

use switchbot::{cli, commands, config, feedback};

fn main() {
    let exit_code = match run() {
        Ok(()) => 0,
        Err(()) => 1,
    };
    std::process::exit(exit_code);
}

/// 戻り値の Err は単に「失敗した」を意味する。詳細メッセージは feedback で出力済み。
fn run() -> Result<(), ()> {
    // 1) 引数パース前にロードできる範囲で log_path だけ取得しておく
    //    (引数バリデーション失敗時にも log/notify を出すため)
    let log_path = config::config_dir().ok().map(|d| d.join("log"));

    // 2) 引数パース。失敗時は help/version は通常通り、それ以外は feedback 経由で通知。
    let cli = match cli::Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // help/version の意図的な表示は exit 0 で stdout に出す (notify しない)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                e.print().ok();
                return Ok(());
            }
            // 引数なし起動: usage を表示して exit 1。notify は不要 (画面に help が見える)。
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) {
                e.print().ok();
                return Err(());
            }
            // それ以外の引数バリデーションエラー: stderr + log + notify
            let msg = e.to_string();
            if let Some(ref lp) = log_path {
                feedback::log_error(lp, &msg);
            }
            feedback::notify(&msg);
            eprintln!("{}", msg);
            return Err(());
        }
    };

    // 3) Context をロード。Setup 失敗は stderr のみ、Runtime 失敗は stderr + log + notify。
    let ctx = match config::load_context() {
        Ok(c) => c,
        Err(e) => {
            let msg = e.to_string();
            if e.should_notify() {
                // Runtime エラー: stderr + log + notify
                if let Some(ref lp) = log_path {
                    feedback::log_error(lp, &msg);
                }
                feedback::notify(&msg);
            }
            eprintln!("{}", msg);
            return Err(());
        }
    };

    // 4) コマンドを実行。成功時はログ INFO、失敗時はログ ERROR + 通知 + stderr。
    match commands::handle(&cli.command, &ctx) {
        Ok(msg) => {
            match cli.command {
                cli::Command::List => {
                    use std::io::Write as _;
                    print!("{}", msg);
                    let _ = std::io::stdout().flush();
                    feedback::log_info(&ctx.log_path, "list ok");
                }
                cli::Command::Status => {
                    println!("{}", msg);
                    feedback::log_info(&ctx.log_path, "status ok");
                }
                cli::Command::Mode => {
                    println!("{}", msg);
                    feedback::log_info(&ctx.log_path, &format!("mode ok ({})", msg));
                }
                _ => {
                    feedback::log_info(&ctx.log_path, &msg);
                }
            }
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            feedback::log_error(&ctx.log_path, &msg);
            feedback::notify(&msg);
            eprintln!("{}", msg);
            Err(())
        }
    }
}
