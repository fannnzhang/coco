use owo_colors::OwoColorize;

use crate::runner::RunSummary;

pub fn print_completion_summary(
    kind: &str,
    run_id: Option<&str>,
    summary: &RunSummary,
    verbose: bool,
) {
    if let Some(id) = run_id {
        println!(
            "{} `{}` completed {} step(s); resume_pointer={}",
            kind_label(kind),
            id,
            summary.executed_steps,
            summary.resume_pointer
        );
    } else {
        println!(
            "{} completed {} step(s); resume_pointer={}",
            kind_label(kind),
            summary.executed_steps,
            summary.resume_pointer
        );
    }

    if verbose {
        print_verbose_line(kind, summary);
    }
}

fn kind_label(kind: &str) -> String {
    format!("[{kind}]").dimmed().to_string()
}

fn print_verbose_line(kind: &str, summary: &RunSummary) {
    let last_completed = if summary.resume_pointer == 0 {
        "n/a".to_string()
    } else {
        summary.resume_pointer.to_string()
    };
    let token_text = if let Some(usage) = &summary.token_usage {
        format!(
            "token_delta(prompt={} completion={} total={} cost=${:.6})",
            usage.prompt_tokens, usage.completion_tokens, usage.total_tokens, usage.total_cost
        )
    } else {
        "token_delta(prompt=0 completion=0 total=0 cost=$0.000000)".to_string()
    };
    println!(
        "{} {} last_completed_step={} resume_pointer={} {}",
        kind_label(kind),
        "summary".bold(),
        last_completed,
        summary.resume_pointer,
        token_text
    );
}
