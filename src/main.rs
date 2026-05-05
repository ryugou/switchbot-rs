use clap::Parser;

use switchbot::{cli, commands, config, feedback};

fn main() {
    let cli = cli::Cli::parse();
    let exit_code = match run(&cli) {
        Ok(()) => 0,
        Err(()) => 1,
    };
    std::process::exit(exit_code);
}

/// 戻り値の Err は単に「失敗した」を意味する。詳細メッセージは feedback で出力済み。
fn run(cli: &cli::Cli) -> Result<(), ()> {
    // 1) Context をロード。失敗 (HOME 不在、初回 bootstrap) は stderr のみ。通知/ログには出さない。
    let ctx = match config::load_context() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            return Err(());
        }
    };

    // 2) コマンドを実行。成功時はログ INFO、失敗時はログ ERROR + 通知 + stderr。
    match commands::handle(&cli.command, &ctx) {
        Ok(msg) => {
            feedback::log_info(&ctx.log_path, &msg);
            // list は出力を stdout にも流す
            if let cli::Command::List = cli.command {
                print!("{}", msg);
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
