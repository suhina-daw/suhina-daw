// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

#[test]
fn every_stock_sd_imports_and_persists_its_definition() {
    let folder = std::env::temp_dir().join(format!("suhina-stock-{}", std::process::id()));
    suhina_device_bundle::write_to(&folder).unwrap();
    for stock in suhina_device_bundle::DEVICES {
        let project = Project {
            tracks: vec![Track {
                content: ContentKinds { notes: stock.instrument, audio: !stock.instrument },
                channel: Some(ChannelId(1)),
                ..Track::new(TrackId(1), "Track")
            }],
            channels: vec![Channel::new(ChannelId(1), "Track")],
            ..Project::default()
        };
        let mut session = Session::new(project);
        let path = folder.join(format!("{}.sd", stock.id));
        session.execute("device.addSd", &json!({"track":1,"path":path})).unwrap();
        let device = &session.project().channels[0].devices[0];
        assert_eq!(device.kind, DeviceKind::Builtin(stock.id.into()));
        assert!(device.sd_definition().is_some());
        let restored = suhina_project_file::from_json(&suhina_project_file::to_json(session.project())).unwrap();
        assert_eq!(&restored, session.project());
        if stock.id == "sampler" {
            let mut project = session.project().clone();
            project.channels[0].devices[0].set_sample_file("source.wav");
            session = Session::new(project);
            let mut newer = stock.graph().unwrap();
            newer
                .migrations
                .push(suhina_device_sdk::DeviceMigration { from_version: newer.device_version, parameter_ids: vec![] });
            newer.device_version += 1;
            std::fs::write(&path, newer.to_sd().unwrap()).unwrap();
            session.execute("device.updateSd", &json!({"track":1,"device":0,"path":path})).unwrap();
            assert_eq!(session.project().channels[0].devices[0].sample_file(), Some("source.wav"));
        }
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(folder).unwrap();
}
