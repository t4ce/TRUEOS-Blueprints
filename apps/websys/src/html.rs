use crate::tree::{RecordKeyLabel, TreeNode, encode_path_segments};
use file_system::jobs::{JobSnapshot, JobStatus};
use file_system::path::Path;

pub const STYLE_PATH: &str = "/ui/style.css";

pub fn stylesheet() -> &'static str {
    include_str!("../assets/ui.css")
}

#[derive(Clone, Copy)]
pub struct TreeLinks {
    pub scope_label: &'static str,
    pub tree_base: &'static str,
    pub file_base: &'static str,
    pub jobs_base: &'static str,
    pub sha_base: &'static str,
}

pub fn render_root_overview_page(app_root: &Path, common_root: Option<&Path>) -> String {
    let body = format!(
        r#"
<div class="shell">
  <section class="header-card">
    <div class="header-card__top">
      <div class="brand">
        <div class="brand__mark" aria-hidden="true">⬡</div>
        <div>
          <p class="eyebrow">TRUEOS File System</p>
          <h1>File roots</h1>
          <p class="subtitle">Choose the capability root to browse or operate on.</p>
        </div>
      </div>
      <div class="path-bar">trueos://</div>
    </div>
    <div class="header-card__nav">
      <a class="{root_class}" href="/">Root</a>
      <a class="{app_class}" href="/tree">App root</a>
      {common_nav}
    </div>
  </section>

  <section class="panel section-gap">
    <div class="panel__header">
      <h2>File tree</h2>
      <p>Top-level filesystem capabilities exposed to this app.</p>
    </div>
    <div class="panel__body">
      <p class="tree-stats">{folder_count} folders · 0 files</p>
      <ul class="tree">
        <li class="tree-item">
          <div class="tree-row">
            <span class="tree-icon tree-icon--folder" aria-hidden="true">📁</span>
            <span class="tree-name">app://</span>
            <span class="tree-meta">{app_path}</span>
            <div class="tree-actions"><a class="action-link" href="/tree">Open</a></div>
          </div>
        </li>
        {common_row}
      </ul>
    </div>
  </section>

  <footer class="footer">App and common roots are separate file capabilities.</footer>
</div>
"#,
        root_class = nav_link_class(true),
        app_class = nav_link_class(false),
        common_nav = common_root
            .map(|_| format!(
                r#"<a class="{}" href="/common/tree">Common root</a>"#,
                nav_link_class(false)
            ))
            .unwrap_or_default(),
        folder_count = if common_root.is_some() { 2 } else { 1 },
        app_path = escape_html(&app_root.display().to_string()),
        common_row = render_common_root_row(common_root),
    );

    render_document("File roots", &body, None)
}

pub fn render_tree_page(
    current: &Path,
    rel: &str,
    nodes: &[TreeNode],
    jobs: &[JobSnapshot],
    links: TreeLinks,
) -> String {
    let title = if rel.is_empty() {
        "File tree".to_string()
    } else {
        format!("{rel} · File tree")
    };
    let body = render_tree_body(current, rel, nodes, jobs, links);
    render_document(&title, &body, None)
}

pub fn render_jobs_page(root: &Path, jobs: &[JobSnapshot], links: TreeLinks) -> String {
    let body = format!(
        r#"
<div class="shell">
  <section class="header-card">
    <div class="header-card__top">
      <div class="brand">
        <div class="brand__mark" aria-hidden="true">⌘</div>
        <div>
          <p class="eyebrow">TRUEOS File System</p>
          <h1>Asynchronous Job Queue</h1>
          <p class="subtitle">Background file operations are queued and executed by the backend worker.</p>
        </div>
      </div>
      <div class="path-bar">{root_path}</div>
    </div>
    <div class="header-card__nav">
      <a class="pill-link" href="{tree_base}">Back to tree</a>
    </div>
  </section>

  <section class="panel section-gap">
    <div class="panel__header">
      <h2>Recent jobs</h2>
      <p>Queue status across file, archive, extraction, and download tasks.</p>
    </div>
    <div class="panel__body">
      {jobs_html}
    </div>
  </section>

  <footer class="footer">Shared CSS and asynchronous worker enabled under S002.</footer>
</div>
"#,
        root_path = escape_html(&root.display().to_string()),
        tree_base = escape_attr(links.tree_base),
        jobs_html = render_jobs_list(jobs, true, links),
    );

    render_document("Job queue", &body, None)
}

pub fn render_job_page(root: &Path, job: &JobSnapshot, links: TreeLinks) -> String {
    let body = format!(
        r#"
<div class="shell">
  <section class="detail-card">
    <div class="header-card__top">
      <div class="brand">
        <div class="brand__mark" aria-hidden="true">⚙</div>
        <div>
          <p class="eyebrow">Background task</p>
          <h1>Job #{job_id}</h1>
          <p class="subtitle">{summary}</p>
        </div>
      </div>
      <div class="path-bar">{root_path}</div>
    </div>
    <div class="header-card__nav">
      <a class="pill-link" href="{jobs_base}">All jobs</a>
      <a class="action-link action-link--secondary" href="{tree_base}">Back to tree</a>
    </div>
    <div class="panel__body">
      <span class="status-badge status-badge--{status_class}">{status_label}</span>
      <dl class="job-detail-grid">
        <dt>Operation</dt>
        <dd>{operation}</dd>
        <dt>Summary</dt>
        <dd>{summary}</dd>
        <dt>Detail</dt>
        <dd class="mono">{detail}</dd>
        <dt>Result path</dt>
        <dd>{result_path}</dd>
      </dl>
      {message_html}
    </div>
  </section>

  <footer class="footer">Active jobs auto-refresh every 2 seconds until completion.</footer>
</div>
"#,
        job_id = job.id,
        summary = escape_html(&job.summary),
        root_path = escape_html(&root.display().to_string()),
        jobs_base = escape_attr(links.jobs_base),
        tree_base = escape_attr(links.tree_base),
        status_class = job.status_class(),
        status_label = job.status_label(),
        operation = job.kind.label(),
        detail = escape_html(&job.detail),
        result_path = render_result_path(job, links),
        message_html = render_job_message(job, links),
    );

    let refresh = if job.status.is_terminal() {
        None
    } else {
        Some(2)
    };
    render_document(&format!("Job #{}", job.id), &body, refresh)
}

fn render_tree_body(
    current: &Path,
    rel: &str,
    nodes: &[TreeNode],
    jobs: &[JobSnapshot],
    links: TreeLinks,
) -> String {
    let title = if rel.is_empty() {
        format!("{} root", links.scope_label)
    } else {
        format!("{} · {rel}", links.scope_label)
    };
    let subtitle = if rel.is_empty() {
        "Browse files and dispatch background operations inside this capability.".to_string()
    } else {
        format!("Current relative path: /{rel}")
    };
    let (dir_count, file_count) = count_nodes(nodes);

    format!(
        r#"
<div class="shell">
  <section class="header-card">
    <div class="header-card__top">
      <div class="brand">
        <div class="brand__mark" aria-hidden="true">⬡</div>
        <div>
          <p class="eyebrow">TRUEOS File System</p>
          <h1>{title}</h1>
          <p class="subtitle">{subtitle}</p>
        </div>
      </div>
      <div class="path-bar">{current_path}</div>
    </div>
    <div class="header-card__nav">
      <a class="{root_class}" href="/">Root</a>
      <a class="{app_class}" href="/tree">App root</a>
      <a class="{common_class}" href="/common/tree">Common root</a>
      {parent_link}
      <a class="action-link action-link--secondary" href="{jobs_base}">View job queue</a>
    </div>
  </section>

  <div class="layout">
    <section class="panel">
      <div class="panel__header">
        <h2>File tree</h2>
        <p>Select Archive or Download for multi-select. File keys come from TRUEOSFS record headers.</p>
      </div>
      <div class="panel__body">
        {selection_toolbar}
        {stats_html}
        {tree_html}
      </div>
    </section>

    <div class="stack">
      <section class="panel">
        <div class="panel__header">
          <h2>Dispatch operations</h2>
          <p>These forms enqueue background jobs instead of blocking the request path.</p>
        </div>
        <div class="panel__body">
          {forms_html}
        </div>
      </section>

      <section class="panel">
        <div class="panel__header">
          <h2>Recent jobs</h2>
          <p>The backend worker processes one job at a time from the queue.</p>
        </div>
        <div class="panel__body">
          {jobs_html}
        </div>
      </section>
    </div>
  </div>

  <footer class="footer">Shared visual tokens are served from one reusable CSS file.</footer>
</div>
"#,
        title = escape_html(&title),
        subtitle = escape_html(&subtitle),
        current_path = escape_html(&current.display().to_string()),
        root_class = nav_link_class(false),
        app_class = nav_link_class(links.tree_base == "/tree"),
        common_class = nav_link_class(links.tree_base == "/common/tree"),
        parent_link = render_parent_link(rel, links),
        jobs_base = escape_attr(links.jobs_base),
        stats_html = render_tree_stats(dir_count, file_count),
        selection_toolbar = render_selection_toolbar(rel, links),
        tree_html = render_tree(nodes, links),
        forms_html = render_job_forms(rel, links),
        jobs_html = render_jobs_list(jobs, false, links),
    )
}

fn render_common_root_row(common_root: Option<&Path>) -> String {
    let Some(common_root) = common_root else {
        return String::new();
    };
    format!(
        r#"
        <li class="tree-item">
          <div class="tree-row">
            <span class="tree-icon tree-icon--folder" aria-hidden="true">📁</span>
            <span class="tree-name">common://</span>
            <span class="tree-meta">{path}</span>
            <div class="tree-actions"><a class="action-link" href="/common/tree">Open</a></div>
          </div>
        </li>
"#,
        path = escape_html(&common_root.display().to_string())
    )
}

fn render_tree(nodes: &[TreeNode], links: TreeLinks) -> String {
    if nodes.is_empty() {
        return r#"<div class="empty-state"><span class="empty-state__icon">📂</span><p>This directory is empty.</p></div>"#
            .to_string();
    }

    let mut tree = String::from(r#"<ul class="tree">"#);
    for node in nodes {
        render_node(&mut tree, node, links);
    }
    tree.push_str("</ul>");
    tree
}

fn render_job_forms(rel: &str, links: TreeLinks) -> String {
    let upload_dir = escape_attr(rel);
    let jobs_base = links.jobs_base.trim_end_matches('/');
    format!(
        r#"
<div class="stack">
  <form class="form-card form-grid" action="{jobs_base}/move" method="post">
    <div>
      <h3>Move</h3>
      <p>Queue a rename or move operation inside the configured root.</p>
    </div>
    <div class="field">
      <label for="move-source">Source path</label>
      <input id="move-source" type="text" name="source" placeholder="demo-data/docs/intro.md" required>
    </div>
    <div class="field">
      <label for="move-destination">Destination path</label>
      <input id="move-destination" type="text" name="destination" placeholder="demo-data/archive/intro.md" required>
    </div>
    <button type="submit">Queue move job</button>
  </form>

  <form class="form-card form-grid" action="{jobs_base}/delete" method="post">
    <div>
      <h3>Delete</h3>
      <p>Remove a file or directory through the background worker.</p>
    </div>
    <div class="field">
      <label for="delete-target">Target path</label>
      <input id="delete-target" type="text" name="target" placeholder="demo-data/logs/http.log" required>
    </div>
    <button type="submit">Queue delete job</button>
  </form>

  <form class="form-card form-grid" action="{jobs_base}/upload" method="post" enctype="multipart/form-data">
    <div>
      <h3>Upload</h3>
      <p>Upload a file and let the queue write it into the selected directory.</p>
    </div>
    <div class="field">
      <label for="upload-directory">Target directory</label>
      <input id="upload-directory" type="text" name="directory" value="{upload_dir}" placeholder="demo-data/uploads">
    </div>
    <div class="field">
      <label for="upload-file">File</label>
      <input id="upload-file" type="file" name="file" required>
    </div>
    <button type="submit">Queue upload job</button>
  </form>

  <form class="form-card form-grid" action="{jobs_base}/download" method="post">
    <div>
      <h3>Download</h3>
      <p>Prepare a staged download copy in the hidden job download area.</p>
    </div>
    <div class="field">
      <label for="download-source">Source file path</label>
      <input id="download-source" type="text" name="source" placeholder="demo-data/README.txt" required>
    </div>
    <button type="submit">Queue download job</button>
  </form>

  <form class="form-card form-grid" action="{jobs_base}/extract" method="post">
    <div>
      <h3>Extract 7z</h3>
      <p>Unpack a native 7z archive into a directory inside this capability.</p>
    </div>
    <div class="field">
      <label for="extract-archive">Archive path</label>
      <input id="extract-archive" type="text" name="archive" placeholder="archives/project.7z" required>
    </div>
    <div class="field">
      <label for="extract-destination">Destination directory</label>
      <input id="extract-destination" type="text" name="destination" value="{upload_dir}" placeholder="projects/restored">
    </div>
    <button type="submit">Queue extract job</button>
  </form>

  <p class="form-note">All paths are relative to the configured root. Parent traversal with <code>..</code> is rejected.</p>
</div>
"#,
        upload_dir = upload_dir,
        jobs_base = escape_attr(jobs_base),
    )
}

fn render_selection_toolbar(rel: &str, links: TreeLinks) -> String {
    let jobs_base = escape_attr(links.jobs_base.trim_end_matches('/'));
    let current_dir = escape_attr(rel);
    format!(
        r#"
<div class="selection-workbench" data-selection-workbench>
  <div class="selection-mode-bar" role="group" aria-label="File explorer mode">
    <span class="selection-mode-label">Explorer mode</span>
    <button class="mode-button" type="button" data-selection-mode="archive">Archive</button>
    <button class="mode-button" type="button" data-selection-mode="download">Download</button>
    <button class="mode-button mode-button--quiet" type="button" data-selection-cancel hidden>Cancel</button>
  </div>
  <div class="selection-action" data-selection-action="archive" hidden>
    <form class="selection-form" action="{jobs_base}/archive" method="post" data-selection-form>
      <input type="hidden" name="sources" value="[]" data-selection-value>
      <div class="field selection-field">
        <label for="archive-directory">Store path</label>
        <input id="archive-directory" type="text" name="directory" value="{current_dir}" placeholder="archives" required title="Choose a directory inside this file-system root">
      </div>
      <div class="field selection-field">
        <label for="archive-name">Archive name</label>
        <input id="archive-name" type="text" name="name" value="archive" required>
      </div>
      <button type="submit" disabled data-selection-submit>Create 7z</button>
      <p class="form-note">A store directory is required; an empty path will not write into the capability root.</p>
    </form>
  </div>
  <div class="selection-action" data-selection-action="download" hidden>
    <form class="selection-form" action="{jobs_base}/download-selection" method="post" data-selection-form>
      <input type="hidden" name="sources" value="[]" data-selection-value>
      <p>Selected files and directories are packed into one 7z download.</p>
      <button type="submit" disabled data-selection-submit>Prepare download</button>
    </form>
  </div>
  <p class="selection-summary" data-selection-summary hidden>0 selected</p>
</div>
"#
    )
}

fn render_jobs_list(jobs: &[JobSnapshot], full_page: bool, links: TreeLinks) -> String {
    if jobs.is_empty() {
        return r#"<p class="jobs-note">No jobs have been submitted yet.</p>"#.to_string();
    }

    let mut output = String::from(r#"<div class="jobs">"#);
    for job in jobs {
        output.push_str(&format!(
            r#"
<article class="job-item">
  <div class="job-item__top">
    <div class="job-item__meta">
      <strong>{operation}</strong>
      <span class="job-id">Job #{id}</span>
      <p class="job-summary">{summary}</p>
    </div>
    <span class="status-badge status-badge--{status_class}">{status_label}</span>
  </div>
  <div class="mono">{detail}</div>
  {message}
  <a class="action-link action-link--secondary" href="{jobs_base}/{id}">Open details</a>
</article>
"#,
            operation = job.kind.label(),
            id = job.id,
            jobs_base = escape_attr(links.jobs_base.trim_end_matches('/')),
            summary = escape_html(&job.summary),
            status_class = job.status_class(),
            status_label = job.status_label(),
            detail = escape_html(&job.detail),
            message = render_job_message(job, links),
        ));
    }
    if !full_page {
        output.push_str(&format!(
            r#"<a class="pill-link" href="{}">Open full queue view</a>"#,
            escape_attr(links.jobs_base)
        ));
    }
    output.push_str("</div>");
    output
}

fn render_job_message(job: &JobSnapshot, links: TreeLinks) -> String {
    if let Some(error) = &job.error {
        return format!(
            r#"<div class="message message--error"><strong>Execution error:</strong> {}</div>"#,
            escape_html(error)
        );
    }

    if let Some(path) = &job.result_path {
        let safe_path = escape_attr(&serve_href(path, links));
        let label = escape_html(path);
        return format!(
            r#"<div class="message message--success"><strong>Result:</strong> <a href="{safe_path}">{label}</a></div>"#
        );
    }

    match job.status {
        JobStatus::Queued => {
            r#"<div class="message"><strong>Queued:</strong> waiting for the background worker.</div>"#
                .to_string()
        }
        JobStatus::Running => {
            r#"<div class="message"><strong>Running:</strong> the background worker is executing this task.</div>"#
                .to_string()
        }
        JobStatus::Succeeded => {
            r#"<div class="message message--success"><strong>Completed:</strong> no additional artifact path was produced.</div>"#
                .to_string()
        }
        JobStatus::Failed => String::new(),
    }
}

fn render_result_path(job: &JobSnapshot, links: TreeLinks) -> String {
    if let Some(path) = &job.result_path {
        let safe_href = escape_attr(&serve_href(path, links));
        let safe_label = escape_html(path);
        format!(r#"<a class="mono" href="{safe_href}">{safe_label}</a>"#)
    } else {
        "<span class=\"mono\">n/a</span>".to_string()
    }
}

fn render_parent_link(rel: &str, links: TreeLinks) -> String {
    if rel.is_empty() {
        return String::new();
    }

    let parent = rel.rfind('/').map(|index| &rel[..index]).unwrap_or("");
    let href = if parent.is_empty() {
        links.tree_base.to_string()
    } else {
        format!(
            "{}/{}",
            links.tree_base.trim_end_matches('/'),
            encode_path_segments(parent)
        )
    };
    format!(r#"<a class="pill-link" href="{href}">Parent directory</a>"#)
}

fn nav_link_class(active: bool) -> &'static str {
    if active {
        "pill-link pill-link--active"
    } else {
        "action-link action-link--secondary"
    }
}

fn render_tree_stats(dir_count: usize, file_count: usize) -> String {
    format!(r#"<p class="tree-stats">{dir_count} folders · {file_count} files</p>"#)
}

fn count_nodes(nodes: &[TreeNode]) -> (usize, usize) {
    let mut dirs = 0;
    let mut files = 0;
    for node in nodes {
        match node {
            TreeNode::Dir { children, .. } => {
                dirs += 1;
                let (child_dirs, child_files) = count_nodes(children);
                dirs += child_dirs;
                files += child_files;
            }
            TreeNode::File { .. } => files += 1,
        }
    }
    (dirs, files)
}

fn render_node(output: &mut String, node: &TreeNode, links: TreeLinks) {
    match node {
        TreeNode::Dir {
            name,
            rel_path,
            children,
        } => {
            let href = format!(
                "{}/{}",
                links.tree_base.trim_end_matches('/'),
                encode_path_segments(rel_path)
            );
            output.push_str(r#"<li class="tree-item"><div class="tree-row">"#);
            render_selection_box(output, rel_path, name);
            output.push_str(
                r#"<span class="tree-icon tree-icon--folder" aria-hidden="true">📁</span>"#,
            );
            output.push_str(r#"<span class="tree-name">"#);
            output.push_str(&escape_html(name));
            output.push_str("</span>");
            output.push_str(r#"<div class="tree-actions">"#);
            output.push_str(&format!(
                r#"<a class="action-link" href="{}">Open</a>"#,
                escape_attr(&href)
            ));
            output.push_str("</div></div>");
            if !children.is_empty() {
                output.push_str(r#"<ul>"#);
                for child in children {
                    render_node(output, child, links);
                }
                output.push_str("</ul>");
            }
            output.push_str("</li>");
        }
        TreeNode::File {
            name,
            rel_path,
            url_path,
            record_key,
        } => {
            output.push_str(r#"<li class="tree-item"><div class="tree-row">"#);
            render_selection_box(output, rel_path, name);
            output.push_str(
                r#"<span class="tree-icon tree-icon--file" aria-hidden="true">📄</span>"#,
            );
            output.push_str(r#"<span class="tree-name"><a href=""#);
            output.push_str(&escape_attr(url_path));
            output.push_str(r#"">"#);
            output.push_str(&escape_html(name));
            output.push_str("</a></span>");
            output.push_str(&render_record_key(record_key));
            output.push_str(r#"<div class="tree-actions">"#);
            output.push_str(&format!(
                r#"<button class="action-link sha-button" type="button" data-sha-url="{}/{}" aria-label="Calculate SHA-256 for {}">SHA</button><span class="sha-result mono" data-sha-result aria-live="polite"></span>"#,
                escape_attr(links.sha_base.trim_end_matches('/')),
                encode_path_segments(rel_path),
                escape_attr(name),
            ));
            output.push_str(&format!(
                r#"<a class="action-link" href="{}">Open file</a>"#,
                escape_attr(url_path)
            ));
            output.push_str("</div></div></li>");
        }
    }
}

fn render_selection_box(output: &mut String, rel_path: &str, name: &str) {
    output.push_str(&format!(
        r#"<label class="tree-select" title="Select {}"><input type="checkbox" value="{}" data-tree-select><span aria-hidden="true"></span></label>"#,
        escape_attr(name),
        escape_attr(rel_path),
    ));
}

fn render_record_key(record_key: &RecordKeyLabel) -> String {
    match record_key {
        RecordKeyLabel::Ffa => {
            r#"<span class="key-badge key-badge--ffa" title="Free-for-all record key">🔑 FFA</span>"#
                .to_string()
        }
        RecordKeyLabel::Key { provider, handle } => {
            let short = handle.get(..8).unwrap_or(handle);
            let color = match provider.as_bytes().first().copied().unwrap_or(b'0') % 3 {
                0 => "violet",
                1 => "amber",
                _ => "cyan",
            };
            format!(
                r#"<span class="key-badge key-badge--{color}" title="Provider: {provider} · Handle: {handle}">🔑 {short}</span>"#,
                provider = escape_attr(provider),
                handle = escape_attr(handle),
                short = escape_html(short),
            )
        }
    }
}

fn serve_href(path: &str, links: TreeLinks) -> String {
    let trimmed = path.trim_start_matches('/');
    if links.file_base.is_empty() {
        format!("/{trimmed}")
    } else {
        format!("{}/{}", links.file_base.trim_end_matches('/'), trimmed)
    }
}

fn render_document(title: &str, body: &str, refresh_seconds: Option<u32>) -> String {
    let refresh_meta = refresh_seconds
        .map(|seconds| format!(r#"<meta http-equiv="refresh" content="{seconds}">"#))
        .unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  {refresh_meta}
  <link rel="stylesheet" href="{style_path}">
</head>
<body>
  {body}
  <script>{script}</script>
</body>
</html>"#,
        title = escape_html(title),
        refresh_meta = refresh_meta,
        style_path = STYLE_PATH,
        body = body,
        script = explorer_script(),
    )
}

fn explorer_script() -> &'static str {
    r#"
(() => {
  const workbench = document.querySelector('[data-selection-workbench]');
  const checks = [...document.querySelectorAll('[data-tree-select]')];
  if (workbench && checks.length) {
    const modeButtons = [...workbench.querySelectorAll('[data-selection-mode]')];
    const actions = [...workbench.querySelectorAll('[data-selection-action]')];
    const cancel = workbench.querySelector('[data-selection-cancel]');
    const summary = workbench.querySelector('[data-selection-summary]');
    let mode = null;

    const sync = () => {
      const selected = checks.filter((item) => item.checked).map((item) => item.value);
      workbench.querySelectorAll('[data-selection-value]').forEach((input) => {
        input.value = JSON.stringify(selected);
      });
      workbench.querySelectorAll('[data-selection-submit]').forEach((button) => {
        button.disabled = selected.length === 0;
      });
      summary.textContent = `${selected.length} selected`;
    };

    const setMode = (next) => {
      mode = next;
      document.body.classList.toggle('selection-active', Boolean(mode));
      modeButtons.forEach((button) => button.classList.toggle('mode-button--active', button.dataset.selectionMode === mode));
      actions.forEach((action) => { action.hidden = action.dataset.selectionAction !== mode; });
      cancel.hidden = !mode;
      summary.hidden = !mode;
      if (!mode) checks.forEach((item) => { item.checked = false; });
      sync();
    };

    modeButtons.forEach((button) => button.addEventListener('click', () => setMode(button.dataset.selectionMode)));
    cancel.addEventListener('click', () => setMode(null));
    checks.forEach((item) => item.addEventListener('change', sync));
    sync();
  }

  document.querySelectorAll('[data-sha-url]').forEach((button) => {
    button.addEventListener('click', async () => {
      const result = button.parentElement.querySelector('[data-sha-result]');
      button.disabled = true;
      button.classList.add('sha-button--loading');
      result.textContent = 'Calculating…';
      try {
        const response = await fetch(button.dataset.shaUrl, { headers: { Accept: 'application/json' } });
        if (!response.ok) throw new Error(await response.text() || `HTTP ${response.status}`);
        const payload = await response.json();
        result.textContent = payload.sha256;
        result.title = `${payload.algorithm} · ${payload.bytes} bytes`;
      } catch (error) {
        result.textContent = 'Hash failed';
        result.title = String(error);
      } finally {
        button.disabled = false;
        button.classList.remove('sha-button--loading');
      }
    });
  });
})();
"#
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_attr(value: &str) -> String {
    escape_html(value)
}
