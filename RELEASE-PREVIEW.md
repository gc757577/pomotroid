# Windows 로컬 배포 후보

- 임시 제품명: **Pomotroid Fork Preview**
- 버전: **0.1.0-rc.1**, Windows x64, NSIS, 현재 Windows 사용자 전용 설치
- 식별자: `io.github.gc75.pomotroidfork.preview`
- 설치 실행 파일: `pomotroid-fork-preview.exe`
- 자동 업데이트: 비활성화. 원본 서버 주소·키·업데이트 플러그인·권한 제거. 명령을 직접 호출해도 거부.
- 완전 종료: 트레이 숨기기를 선택하지 않은 X 버튼은 앱 전체 종료를 요청한다. 위젯의 숨겨진 소유 창이 프로세스를 남기던 경로를 보완했다.
- 코드 서명: 없음. 공개 배포 및 자동 업데이트 채널은 아직 준비하지 않음.

## 데이터 분리

| 앱 | 데이터 폴더 |
| --- | --- |
| 원본 Pomotroid | `%APPDATA%\com.splode.pomotroid` |
| 개발 앱 | `%APPDATA%\io.github.gc75.pomotroidfork.dev` |
| 배포 후보 | `%APPDATA%\io.github.gc75.pomotroidfork.preview` |

DB 이름은 각 폴더의 `pomotroid.db`. 원본이 다른 세대의 Pomotroid라면 저장 방식이 다를 수 있으므로 첫 행은 현재 원본 Tauri 식별자 기준이다. 기존 설치 데이터 폴더를 열어 확인하거나 복사하지 않는다.

Tauri의 `app_data_dir()`로 현재 식별자 아래에만 DB·설정·사용자 소리·테마를 생성한다. 시작 시 빌드별 식별자를 검사하고 불일치하면 로그/DB 생성 전에 중단한다. 기록 마이그레이션이나 개발 DB 자동 가져오기는 없다. 설치 리소스에도 DB가 포함되지 않는다.

설치 후 첫 실행에서 기록이 비어 있는지 확인하고, Preview에서 새 기록을 만든 뒤 Preview 데이터 폴더에만 DB가 생기는지 확인한다. 원본 앱이 실행 중이면 자체 기록이 변경될 수 있으므로 파일 시각만으로 보존 여부를 판단하지 않는다. **실제 설치 후 경로 검증은 수동 확인 대기**다.

## 만들기

Windows C++ 빌드 도구·WebView2·Rust stable MSVC·Node.js·Python 3가 필요하다.

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
npm ci
npm run release:licenses
npm run check
node --test tests/*.test.mjs
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings
npm run desktop:build:preview
```

로컬 소켓이 금지된 샌드박스에서는 WebSocket 통합 테스트가 10013으로 실패할 수 있다. 네트워크 권한이 있는 환경에서 동일 코드를 다시 검사한다. 타이머 테스트를 통과시키기 위한 대기 시간 변경은 하지 않았다.

설치 파일: `src-tauri\target\release\bundle\nsis\Pomotroid Fork Preview_0.1.0-rc.1_x64-setup.exe`.
최초 빌드는 NSIS 도구를 내려받는다. WebView2가 없는 PC는 설치 과정에서 Microsoft 런타임 다운로드가 필요하다.

## 설치·실행·완전 종료

1. 위 설치 파일을 더블 클릭하고 **Pomotroid Fork Preview**인지 확인한다.
2. 사용권 고지에 동의하고 설치한다. 원본 Pomotroid를 제거하거나 개발 폴더를 선택할 필요가 없다.
   기본 설치 폴더는 `%LOCALAPPDATA%\Pomotroid Fork Preview`이다.
3. 설치 완료 후 시작 메뉴에서 **Pomotroid Fork Preview**를 검색해 실행한다.
4. 첫 실행 기록은 비어 있다. 설정 → 시스템에서 한국어를 선택할 수 있다.
5. 트레이 관련 옵션을 사용하지 않으면 창의 X로 종료한다. 트레이로 숨기기를 켰다면 알림 영역의 Preview 아이콘을 오른쪽 클릭해 **종료**를 선택한다. 숨겨진 아이콘은 작업 표시줄의 ∧ 안에 있을 수 있다. 이때 바탕화면 위젯도 함께 사라져야 한다.
6. 제거는 Windows 설정 → 앱 → 설치된 앱 → **Pomotroid Fork Preview**에서 한다. 원본·개발 앱 항목은 선택하지 않는다.

## 포함한 고지와 공개 전 점검

설치 폴더 `licenses`에 원본 MIT 전문·저작권 고지, Mona Sans OFL 및 폰트 저작권, Windows Cargo 의존성·설치된 npm 빌드 도구 고지/목록, NSIS 3.11 및 Tauri NSIS 플러그인 고지를 포함한다. MPL 전용 Rust 의존성은 해당 버전 소스 ZIP도 함께 제공한다. 원본 About 소스 링크는 원본 프로젝트임을 명시한다.

- [ ] 이 설치 파일로 신규 설치·실행·완전 종료·재실행·제거 확인
- [ ] 원본·개발 앱과 동시에 설치 가능한지, 각 앱의 기록·설정이 분리되는지 확인
- [ ] 설치된 빌드의 한국어·알림·소리·트레이·불투명도·호버·위젯·창 상태 및 즉시 모드 전환 확인
- [ ] 자정 경과, 실제 PC 절전·복귀, 다중 모니터 연결/해제, 서로 다른 DPI 검증
- [ ] 다른 Windows 버전/빌드 및 WebView2 미설치 환경 검증
- [ ] 같은 Preview 식별자의 후속 버전 덮어쓰기와 데이터 보존 확인
- [ ] 코드 서명·SmartScreen·백신 평판, 정식 이름·아이콘·배포 경로 결정
- [ ] 공개 배포 전 의존성/자산 라이선스 최종 검토. SeaHash 배포본은 원문 파일이 없어 패키지의 MIT 선언·저자 정보와 표준 MIT 전문을 보완했으므로 원저자 고지 추가 여부 확인

기존 개발 앱에서 확인한 동작과 상세 미확인 목록은 `DEVELOPMENT-WINDOWS.md`에 유지한다. 개발 앱의 확인 결과를 설치된 Preview 빌드의 검증 결과로 간주하지 않는다. Git push와 공개 업로드는 수행하지 않는다.
