# NetX 보고서 원자료(JSON)의 모양 (1판)

근거: `docs/REPORT_FEATURE_REQUEST_v1.md` 2.1, 2.6. [보고서 만들기]에서 "원자료 JSON도 함께 저장합니다"를 고르면 보고서 옆에 `<보고서 이름>-data.json`이 생긴다. 만드는 곳은 `src-tauri/src/report.rs`의 `build`.

- 머리: `report`("antioch.report/1"), `program`("netx"), `version`, `generated_at`, `period`(`from`, `to`. 한 건짜리 보고서는 `null`), `masked`, `sections`.
- 시각은 `2026-11-14T05:12:00+09:00` 모양(이 PC의 시간대).
- 구역마다 `id`, `title`, `count`(기간 안 전체 건수), `shown`(실은 건수), `items`.
- 가리기를 켰으면 `items` 안의 글자도 보고서와 똑같이 가린다.
- 글자는 장비·서버·다른 프로그램이 보낸 것일 수 있다. 읽는 쪽은 자료로만 다룬다(명령이나 화면 꼬리표로 쓰지 않는다).

## 구역별 `items`

| 구역 `id` | 한 항목의 모양 |
|---|---|
| `mynet` | 한 건. `iface`, `mac`, `ip`, `prefix`, `gateway`, `dns`(배열), `link`, `public_ip` |
| `history` | `time`(밀리초), `entry`(진단 기록: `meta`, `recipe_id`, `inputs`, `plan`, `run`). 구성도 그림(`topo_svg`)은 뺀다 |
| `topology` | 가장 최근 조사 한 건. `time`(밀리초), `topology`(`cidr`, `gateway`, `me`, `nodes`, `links`) |
| `capture` | `name`, `size`, `modified`(초), 그리고 `analysis`(`packets`, `bytes`, `seconds`, `link`, `protocols`, `talks`, `findings`, `notes`) 또는 못 열었을 때 `error`. 패킷 내용은 없다 |
| `manual` | `time`(밀리초), `command`, `tool`, `risk`, `state`(ok·error·timeout·cancelled·capped), `exit`, `secs`, `output`(앞 40줄), `cut` |
| `terminal` | `time`(초), `kind`, `target`, `user`. 터미널에 오간 글자는 없다(NetX가 저장하지 않는다) |
| `audit` | 감사 로그 한 줄 그대로: `time`(초), `source`, `tool`, `argv`, `risk`, `root`, `confirmed` 등 |

`history`의 `time`과 `manual`의 `time`은 밀리초, `capture`·`terminal`·`audit`는 초다(저장된 그대로).
