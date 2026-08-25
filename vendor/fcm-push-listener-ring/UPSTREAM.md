# fcm-push-listener-ring

A fork of [fcm-push-listener](https://github.com/RandomEngy/fcm-push-listener) patched for Ring doorbell/camera push notifications.

## Changes from upstream

This fork makes two changes to support Ring's FCM registration:

1. **Device type**: Changed from Chrome (3) to Android (2) in GCM checkin
2. **App package**: Changed from `org.chromium.linux` to `com.ringapp`

These changes are required because Ring's FCM backend expects Android device registrations, not Chrome browser registrations.

## Usage

```rust
use fcm_push_listener::{register, Registration, Message, MessageStream};

// Ring's Firebase credentials
let firebase_app_id = "1:876313859327:android:e10ec6ddb3c81f39";
let firebase_project_id = "ring-17770";
let firebase_api_key = "AIzaSyCv-hdFBmmdBBJadNy-TFwB-xN_H5m3Bk8";

let http = reqwest::Client::new();
let registration = register(&http, firebase_app_id, firebase_project_id, firebase_api_key, None).await?;

// Use registration.fcm_token to register with Ring API
// Then listen for push notifications...
```

## License

MIT (same as upstream)
