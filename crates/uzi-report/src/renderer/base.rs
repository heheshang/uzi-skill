//! Port of `lib/pipeline/renderer/base.py` — abstract section renderer and
//! shared `RenderContext`.

use serde_json::{Map, Value};

/// Renderer input context shared by every section.
#[derive(Debug, Clone)]
pub struct RenderContext {
    pub ticker: String,
    pub name: String,
    pub market: String,
    pub data: Value,
    pub meta: Value,
    pub quality: String,
}

impl RenderContext {
    pub fn new(ticker: &str, name: &str) -> Self {
        RenderContext {
            ticker: ticker.to_string(),
            name: name.to_string(),
            market: "A".to_string(),
            data: Value::Object(Map::new()),
            meta: Value::Object(Map::new()),
            quality: "full".to_string(),
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }

    pub fn with_meta(mut self, meta: Value) -> Self {
        self.meta = meta;
        self
    }

    pub fn with_quality(mut self, quality: &str) -> Self {
        self.quality = quality.to_string();
        self
    }
}

/// Every section implements `render_full`; `render_lite`/`render_gap` have
/// upstream defaults.
pub trait SectionRenderer {
    fn section_id(&self) -> &'static str {
        ""
    }

    fn section_title(&self) -> &'static str {
        ""
    }

    fn render(&self, ctx: &RenderContext) -> String {
        let q = if ctx.quality.is_empty() {
            "full"
        } else {
            ctx.quality.as_str()
        };
        match q {
            "error" => self.render_gap(ctx, "fetcher 失败"),
            "missing" => self.render_gap(ctx, "数据未抓到"),
            "partial" => self.render_lite(ctx),
            _ => self.render_full(ctx),
        }
    }

    fn render_full(&self, ctx: &RenderContext) -> String;

    fn render_lite(&self, ctx: &RenderContext) -> String {
        self.render_full(ctx)
    }

    fn render_gap(&self, _ctx: &RenderContext, reason: &str) -> String {
        let id = self.section_id();
        let title = {
            let t = self.section_title();
            if t.is_empty() {
                id
            } else {
                t
            }
        };
        format!(
            r##"<section id="{id}" class="section-gap"><h2>{title}</h2><div class="gap-notice" style="padding:24px;text-align:center;color:#94a3b8;font-size:12px">⚠️ {reason}（{id}）</div></section>"##
        )
    }
}

