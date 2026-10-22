// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Optional CSS geometry for the native browser, project view and detail editor.
use egui::{Rect, Vec2};

#[derive(Clone, Debug)]
pub struct Workspace {
    pub browser: Option<Rect>,
    pub main: Rect,
    pub detail: Option<Rect>,
}
impl Workspace {
    pub fn translated(mut self, offset: Vec2) -> Self {
        self.browser = self.browser.map(|rect| rect.translate(offset));
        self.main = self.main.translate(offset);
        self.detail = self.detail.map(|rect| rect.translate(offset));
        self
    }
}

/// `detail` is (stacked beside the view, native preferred size). Native visibility
/// remains authoritative; CSS may further hide a panel but cannot open closed panels.
pub fn workspace(size: Vec2, browser: Option<f32>, detail: Option<(bool, f32)>) -> Option<Workspace> {
    if !size.is_finite() || size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    super::ENGINE.with(|engine| {
        let mut engine=engine.borrow_mut();
        engine.source.as_ref()?;
        if engine.source!=super::skin::current().css {return None;}
        let key=format!("{size:?}:{browser:?}:{detail:?}");
        if let Some((previous,layout))=&engine.workspace && previous==&key {return layout.clone();}
        let mut body=String::from("<!doctype html><html><body class=\"window\"><div class=\"workspace\" id=\"workspace\">");
        let mut ids=Vec::new();
        if browser.is_some() {ids.push("panel-browser");}
        if detail.is_some() {ids.push("panel-detail");}
        ids.push("panel-main");
        for id in &ids { body.push_str(&format!("<div class=\"workspace-panel\" id=\"{id}\"></div>")); }
        body.push_str("</div></body></html>");
        let width=browser.unwrap_or(0.0).clamp(0.0,4096.0);
        let stacked=detail.is_some_and(|(stacked,_)| stacked);
        let detail_size=detail.map_or(0.0,|(_,size)| size).clamp(0.0,4096.0);
        let (columns,rows,detail_position)=if stacked {
            (format!("{width}px minmax(0,1fr) {detail_size}px"),"minmax(0,1fr)".into(),"grid-column:3;grid-row:1;")
        } else {
            (format!("{width}px minmax(0,1fr)"),format!("minmax(0,1fr) {detail_size}px"),"grid-column:2;grid-row:2;")
        };
        let defaults=format!("html,body {{margin:0;}} .workspace {{display:grid;width:{}px;height:{}px;grid-template-columns:{columns};grid-template-rows:{rows};}} .workspace-panel {{min-width:0;min-height:0;}} #panel-browser {{grid-column:1;grid-row:1 / -1;}} #panel-main {{grid-column:2;grid-row:1;}} #panel-detail {{{detail_position}}}",size.x,size.y);
        let mut doc=super::document(&engine,&body,&defaults)?;
        doc.resolve(0.0);
        let requested=std::iter::once("workspace").chain(ids.iter().copied()).any(|name|doc.get_element_by_id(name).is_some_and(|id|super::requests_layout(&doc,id)));
        let layout=if requested {
            let rectangle=|name:&str| -> Option<Rect> {
                let node=doc.get_node(doc.get_element_by_id(name)?)?;
                if !node.has_boxes() || !super::computed(&doc,node.id)?.visible {return None;}
                let at=node.absolute_position(0.0,0.0);
                let size=node.final_layout().size;
                let rect=Rect::from_min_size(egui::pos2(at.x,at.y),Vec2::new(size.width,size.height));
                (rect.is_finite() && rect.is_positive() && [rect.min.x,rect.min.y,rect.max.x,rect.max.y].iter().all(|v| v.abs()<=16384.0)).then_some(rect)
            };
            Some(Workspace {browser:rectangle("panel-browser"),main:rectangle("panel-main").unwrap_or(Rect::from_min_size(egui::Pos2::ZERO,Vec2::ZERO)),detail:rectangle("panel-detail")})
        } else {None};
        engine.workspace=Some((key,layout.clone()));
        layout
    })
}
