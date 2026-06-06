use regex::Regex;

#[derive(Debug)]
struct Chapter {
    part_title: String,
    chapter_title: String,
    text: String,
}

/// 解析篇标题，返回 (篇名, 去掉 "## " 前缀后的全文)
fn parse_part_heading(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("## 第") && trimmed.contains("篇") {
        Some(trimmed.trim_start_matches("## ").trim().to_string())
    } else {
        None
    }
}

/// 解析章标题，返回去掉 "## " 前缀后的全文。
/// 如果 "## 第X章" 后面没有标题文字（纯数字标题页 OCR 丢失），向前查找标题补全。
/// 课本中空标题章节的模式：`## 第X章` → (空行/图片/作者名) → `## 实际标题`
fn parse_chapter_heading(line: &str, following_lines: &[&str]) -> Option<String> {
    let trimmed = line.trim_start();
    if !(trimmed.starts_with("## 第") && trimmed.contains("章") && !trimmed.contains("篇")) {
        return None;
    }
    let base = trimmed.trim_start_matches("## ").trim();

    // 判断标题是否只有章节编号（如 "第二章"、"第三章"）
    let is_bare = base.chars().all(|c| {
        c == '第' || c == '章' || "一二三四五六七八九十百零".contains(c) || c.is_whitespace()
    });

    if !is_bare {
        return Some(base.to_string());
    }

    // 向前查找实际标题：跳过空行、图片、乱码、作者名（括号开头），找到第一个有效标题行
    for next in following_lines.iter().take(6) {
        let next_trimmed = next.trim();
        let next_body = next_trimmed.trim_start_matches("## ").trim();

        // 跳过空行、图片、乱码标记、作者行（以"（"开头）
        if next_body.is_empty()
            || next_body.starts_with("![")
            || looks_like_garbled(next)
            || next_trimmed.starts_with("（")
        {
            continue;
        }

        // 遇到其他章节/篇/节标题 → 停止，保留原始
        if next_body.starts_with("第")
            && (next_body.contains("篇")
                || next_body.contains("节")
                || (next_body.contains("章") && next_body != base))
        {
            break;
        }

        // 找到有效标题行
        return Some(format!("{base} {next_body}"));
    }

    Some(base.to_string())
}

/// 将课本全文按章切分，跳过前言和目录区域
fn split_by_chapter(full_text: &str) -> Vec<Chapter> {
    let re_part = Regex::new(r"^## 第[一二三四五六七八九]篇 .+$").unwrap();
    let _re_chapter = Regex::new(r"^## 第[一二三四五六七八九十百零]+章 .+$").unwrap();
    let re_image = Regex::new(r"^!\[Image\]\(.+\)$").unwrap();
    // 行内页码伪影，如 "180 第三篇循环系统疾病" 或 "187 第二章心力衰竭"
    let re_page_artifact = Regex::new(r"^\d+\s+第[一二三四五六七八九十百零]+[篇章].+$").unwrap();

    let lines: Vec<&str> = full_text.lines().collect();

    // 找到第一个篇标题的位置，跳过前言/目录
    let first_content_line = lines.iter().position(|l| re_part.is_match(l)).unwrap_or(0);

    // 收集篇标题和章标题的位置
    let mut current_part: String = String::new();
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut chapter_starts: Vec<(usize, String, String)> = Vec::new(); // (line_idx, part, chapter_title)

    for i in first_content_line..lines.len() {
        if let Some(part) = parse_part_heading(lines[i]) {
            current_part = part;
        } else if let Some(ch_title) = parse_chapter_heading(lines[i], &lines[i + 1..]) {
            chapter_starts.push((i, current_part.clone(), ch_title));
        }
    }

    // 切分每章文本
    for (idx, (start, part, ch_title)) in chapter_starts.iter().enumerate() {
        let end = if idx + 1 < chapter_starts.len() {
            // 下一章之前，回溯跳过图片和乱码行
            let mut raw_end = chapter_starts[idx + 1].0;
            while raw_end > *start + 1 {
                let prev = &lines[raw_end - 1];
                if re_image.is_match(prev) || prev.trim().is_empty() {
                    raw_end -= 1;
                } else {
                    break;
                }
            }
            raw_end
        } else {
            lines.len()
        };

        let raw_text: String = lines[*start..end].join("\n");

        // 清洗文本：移除页码伪影行、孤立图片行、乱码标记
        let cleaned: Vec<&str> = raw_text
            .lines()
            .filter(|l| {
                let t = l.trim();
                if t.is_empty() {
                    return true;
                }
                if re_page_artifact.is_match(l) {
                    return false;
                }
                // 跳过章标题行之前的孤立图片和乱码（保留章标题行本身）
                if *l != lines[*start] && (re_image.is_match(l) || looks_like_garbled(l)) {
                    return false;
                }
                true
            })
            .collect();

        let text = cleaned.join("\n").trim().to_string();
        if !text.is_empty() {
            chapters.push(Chapter {
                part_title: part.clone(),
                chapter_title: ch_title.clone(),
                text,
            });
        }
    }

    chapters
}

/// 检测 OCR 乱码伪影（如 "᱙章᪝ Tk源"）
fn looks_like_garbled(s: &str) -> bool {
    let non_ascii: Vec<char> = s.chars().filter(|c| !c.is_ascii() && *c != ' ').collect();
    if non_ascii.len() < 3 {
        return false;
    }
    // 如果包含非中日韩统一字符域的特殊 Unicode，很可能是乱码
    non_ascii.iter().any(|c| {
        let cp = *c as u32;
        // 这些范围是常见 OCR 伪影中出现的罕见 Unicode 字符
        (cp >= 0x1C80 && cp <= 0x1CCF) // 旧匈牙利文字母范围
            || (cp >= 0xA960 && cp <= 0xA97F) // 朝鲜文字母扩展
            || (cp >= 0x1880 && cp <= 0x18AF) // 蒙古文字母扩展
            || (cp >= 0x1700 && cp <= 0x171F) // 塔加拉文字母
    })
}

#[test]
fn test_split_by_chapter() {
    let sample = r#"## 前言

这是前言内容。

## 第一篇 呼吸系统疾病

![Image](artifacts/image_001.png)

᱙章᪝ braces 源

## 第一章 总 论

呼吸系统疾病总论内容。

180 第一篇呼吸系统疾病

## 第一节 | 急性上呼吸道感染

急性上呼吸道感染内容。

## 第二章 急性气管-支气管炎

气管支气管炎内容。

## 第二篇 循环系统疾病

## 第一章 总 论

循环系统总论。

187 第二章心力衰竭

## 第二章 心力衰竭

心力衰竭内容。
"#;

    let chapters = split_by_chapter(sample);
    assert!(!chapters.is_empty());

    // 应该有 4 个章节（节是 section 不是 chapter）
    assert_eq!(chapters.len(), 4, "expected 4 chapters, got {:?}", chapters);

    // 验证第一章
    assert_eq!(chapters[0].part_title, "第一篇 呼吸系统疾病");
    assert_eq!(chapters[0].chapter_title, "第一章 总 论");

    // 验证循环系统
    assert_eq!(chapters[2].part_title, "第二篇 循环系统疾病");
    assert_eq!(chapters[2].chapter_title, "第一章 总 论");
    assert_eq!(chapters[3].chapter_title, "第二章 心力衰竭");

    // 验证页码伪影已被清洗
    assert!(
        !chapters[0].text.contains("180 第一篇呼吸系统疾病"),
        "page artifact should be removed"
    );
}
