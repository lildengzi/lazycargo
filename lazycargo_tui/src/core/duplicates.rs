/// 从 `cargo tree --duplicates` 输出提取顶层重复 crate 行（"name vX.Y.Z"）。
///
/// 完整树输出（含引入路径）原样保留在输出 slot 中；此函数只提炼"哪些 crate
/// 出现了多个版本"的摘要，供状态行与摘要面板使用。
pub fn duplicate_crates(output: &[String]) -> Vec<String> {
    output
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed != line || trimmed.is_empty() {
                return None;
            }
            let mut parts = trimmed.split_whitespace();
            let name = parts.next()?;
            let version = parts.next()?;
            if version.starts_with('v')
                && version.len() > 1
                && version[1..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
            {
                Some(format!("{name} {version}"))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn extracts_top_level_duplicate_crates() {
        let parsed = duplicate_crates(&lines(&[
            "getrandom v0.2.17",
            "└── ring v0.17.14",
            "    ├── rustls v0.23.41",
            "getrandom v0.4.3",
            "└── tempfile v3.27.0",
            "",
        ]));

        assert_eq!(
            parsed,
            vec![
                "getrandom v0.2.17".to_owned(),
                "getrandom v0.4.3".to_owned()
            ]
        );
    }

    #[test]
    fn ignores_indented_and_empty_lines() {
        let parsed = duplicate_crates(&lines(&[
            "  ├── ansi-to-tui v8.0.1",
            "hashbrown v0.16.1",
            "",
            "    └── kasuari v0.4.12",
        ]));

        assert_eq!(parsed, vec!["hashbrown v0.16.1".to_owned()]);
    }
}
