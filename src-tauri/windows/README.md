# ClearPomo Windows 설치 호환성

표시 제품명과 기본 시작 메뉴 폴더는 **ClearPomo**다. 개발 앱에는 구분용 `Dev` 접미사를 붙인다.

기존 Preview 설치를 같은 앱으로 업데이트하기 위해 다음 내부 식별 정보는 유지한다.

| 항목 | 유지하는 값 |
| --- | --- |
| Tauri 식별자/데이터 폴더 | `io.github.gc75.pomotroidfork.preview` |
| 개발 식별자/데이터 폴더 | `io.github.gc75.pomotroidfork.dev` |
| 실행 파일명 | `pomotroid-fork-preview.exe` |
| 설치/제거 레지스트리 키 이름 | `Pomotroid Fork Preview` |
| 설치 경로 저장 키 | `Software\Pomotroid Fork\Pomotroid Fork Preview` |
| 자동 실행 Run/StartupApproved 값 이름 | `io.github.gc75.pomotroidfork.preview` |

Preview라는 문자열은 위 내부 호환 키·실행 파일 경로와 이전 바로가기를 찾는 코드에만 남긴다. 표시 이름은 `DisplayName`, 창 제목, 정보 화면, 설치/제거 프로그램, 새 바로가기에서 ClearPomo를 사용한다. 기존 설치 폴더의 물리적 이름도 그대로 복원하므로, 폴더 경로에 Preview가 남을 수 있다. 공부 기록을 이동하거나 복사하지 않는다.

## 공식 NSIS 템플릿의 최소 수정

`installer.nsi`는 설치된 Tauri CLI **2.11.2**의 공식 템플릿을 가져와 원본 출처/라이선스 주석과 `INSTALLIDENTITY`를 추가하고, 제품명으로 만들던 설치/제거 키 두 곳을 고정 식별자로 바꾼 파일이다.

원본: https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.2/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi

Tauri MIT 전문은 `licenses/installer/tauri-template-LICENSE.txt`에 보존하고 배포 고지 리소스에 포함한다. CLI 버전을 올릴 때에는 이 템플릿을 재검토해야 하며 관련 검사가 이를 확인한다.

기존 설치 위치와 버전을 같은 키에서 읽으므로 Preview를 제거하지 않고 업데이트할 수 있다. 설치 후에는 같은 제거 항목의 DisplayName을 ClearPomo로 기록한다. 자동 실행의 설치 위치 검증도 동일한 내부 제거 키를 사용한다. Run 경로를 갱신해도 Windows의 StartupApproved 비활성화 바이트는 수정하지 않는다.

`nsis-hooks.nsh`는 기존 데스크톱/기본 시작 메뉴의 Preview 바로가기 중 실제 대상이 이 앱인 것만 새 이름으로 바꾼다. 새 바로가기와의 충돌이나 생성 실패가 있으면 이전 바로가기를 지우지 않는다. 원본 Pomotroid의 바로가기나 데이터를 대상으로 하지 않는다. 사용자 지정 시작 메뉴 폴더·작업 표시줄 고정 항목·Windows 아이콘 캐시의 실제 갱신은 다음 설치 후 확인해야 한다.

## 2026-11-10 검사와 미확인 사항

- 타입 오류/경고 0개, JavaScript 21개, Rust 122개 통과(실등록 시험 1개는 기본 제외), Clippy `-D warnings`, 프런트엔드 프로덕션 빌드 통과.
- PNG RGBA/모서리 투명·ICO 다중 해상도, 트레이의 라운드 색상/남은·경과 진행률/일시정지, 기존 공부 기록 보존 회귀 검사 통과.
- Windows용 테스트 실행 파일의 ProductName/FileDescription이 ClearPomo로 생성됨을 확인했다.
- 바로가기 보조 함수는 대체한 최소 NSIS 스크립트로 훅의 전처리 구문을 확인했다. 실제 바로가기/레지스트리는 조작하지 않았다.
- 이번에는 설치 파일을 만들거나 실행하지 않았다. 실제 Preview → ClearPomo 업데이트, 설치/제거 키·바로가기/고정 아이콘 갱신, 자동 실행 유지 및 Windows 로그인은 다음 설치 파일에서 수동 확인해야 한다.
- 설치된 Preview rc.2의 로그인 자동 실행을 사용자가 확인한 기존 결과는 유지한다. ClearPomo 이름으로 설치된 앱의 확인 결과로 확대하지 않는다.
