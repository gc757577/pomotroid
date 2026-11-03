# 바탕화면 위젯 부착 기술 검증

2026-11-01. 목표는 **기존 통계·잔디의 바탕화면 상시 표시**이다. 일반 통계 창 추가를 대안으로 삼지 않는다. 이번 변경은 통계 연결 전 최소 시험 창까지이며 push하지 않는다.

이 문서의 기술 검증 결과는 커밋 `294aea7`로 보존했다. 후속 실제 통계 위젯에서는 숨김 Tauri 소유 창과 교체 가능한 네이티브 표면·자식 WebView를 분리하여, 아래에 기록한 창 메타데이터 누적 문제를 해결했다. 현재 구현과 검증은 [Windows 개발 문서의 실제 통계 위젯 절](DEVELOPMENT-WINDOWS.md)을 참고한다. 아래 내용은 실패 경로와 당시 판정을 포함한 조사 기록이다.

## 이전 시도와 원인

| 시도 | 확인한 사실 | 판정 / 미확정 사항 |
| --- | --- | --- |
| Win32 `STATIC` 창을 아이콘 호스트의 자식으로 연결 | 부모 HWND 일치, 포인터 hit-test가 자기 버튼을 반환. 일반 창에서는 사용자가 클릭 증가 확인 | 부착 뒤 버튼·글자가 보이지 않는 사용자 화면 확인. **표시 실패**. 부모·입력 판정 성공은 실제 합성·표시 성공을 보장하지 않음 |
| 위 창을 연결한 뒤 layered 스타일과 alpha 255 설정 | `SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA)`가 0 반환, `GetLastError()` 87. `SetParent` 오류가 아님. 일반 창 복구 로그 확인 | **API 단계 실패**. 뒤이은 Win+D로 일반 창이 숨겨졌을 가능성은 있으나 사용자 화면 부재의 유일한 원인으로 확정하지 않음 |
| 실패한 호스트 구성을 차단 | 호스트 exstyle `0x200080`, `WS_EX_NOREDIRECTIONBITMAP` 포함 | 임시 재실패 방지였으며 Windows 바탕화면 부착 자체가 불가능하다는 결론은 아님 |

실행 파일의 RT_MANIFEST를 Windows SDK `mt.exe`로 추출했으며, 기존 것은 Common Controls v6 선언만 있고 supportedOS 선언은 없었다. [Microsoft 문서](https://learn.microsoft.com/en-us/windows/win32/winmsg/using-windows)는 layered **자식** 창에 Windows 8 이상 호환 선언을 요구한다.

숨김 테스트 HWND를 쓰는 독립 실행 파일에서 동일한 자식 창과 API 호출을 사용하고 매니페스트만 바꿨다. 기준 매니페스트도 기존 실행 파일에서 추출한 Common Controls v6를 사용했다.

| 매니페스트 | `SetLayeredWindowAttributes` 반환 | 오류 |
| --- | --- | --- |
| 기존 Common Controls v6만 | 0 | 87 |
| 같은 선언 + Windows 8/10 supportedOS | 1 | 0 |

따라서 오류 87의 호환 선언 누락 원인은 **재현 비교로 확인**했다. 최초 GDI 창이 안 보였던 합성 경로의 정확한 원인은 아직 확정하지 않았다. 최신 Explorer의 리디렉션 없는 부모 구성이 관련됐다는 것은 공개 구현과 관찰에 근거한 추정이다. 이전 시험이 실제 Tauri WebView가 아니었다는 한계도 있었다.

진단 코드·이전 네이티브 구현·추출 매니페스트는 무시되는 `src-tauri/target/desktop-research`에 보관했다. 원본 Pomotroid 데이터는 진단에 사용하지 않았다.

## 공개 코드 비교

| 공개 구현 / 조사 리비전 | 실제 코드에서 확인한 방법 | 현재 목표에 대한 판단 |
| --- | --- | --- |
| [Lively WinDesktopCore](https://github.com/lively-community/lively/blob/25a0a5f4ed1988ff42117d3a17feb321062237fe/src/Lively/Lively/Core/WinDesktopCore.cs) | 최신 Progman 구성 감지, layered 불투명 표면 준비 **후** SetParent, 아이콘 뷰 아래 배치. Explorer 이벤트로 재설정 | 최신 Explorer를 다루는 렌더링 순서의 근거. 배경화면과 달리 이 위젯은 직접 입력을 받아야 하므로 작은 사각형을 아이콘 뷰 위에 배치하도록 변경 |
| [tauri-plugin-wallpaper attacher](https://github.com/meslzy/tauri-plugin-wallpaper/blob/27802478e9b2a7c59901d6411ecfdb74aa240a7f/src/platform/windows/attacher.rs), [input](https://github.com/meslzy/tauri-plugin-wallpaper/blob/27802478e9b2a7c59901d6411ecfdb74aa240a7f/src/platform/windows/input.rs), [reattach](https://github.com/meslzy/tauri-plugin-wallpaper/blob/27802478e9b2a7c59901d6411ecfdb74aa240a7f/src/platform/windows/reattach.rs) | WorkerW 탐색, Tauri WebView 부모 변경, TaskbarCreated 감시. 배경화면 입력은 전역 raw input으로 전달 | MIT 라이선스로 활용 가능. 조사한 4.0.0은 Tauri ^2.11.5를 요구하며 현재 프로젝트는 2.11.2. 아이콘 뒤 배치와 입력 전달은 작은 직접 조작 위젯에 불필요해 이번에는 의존성 추가·Tauri 업데이트를 하지 않음 |
| [Rainmeter Skin](https://github.com/rainmeter/rainmeter/blob/54df7737c615ebe5a6b1d63e539aec171b54aec7/Library/Skin.cpp), [System](https://github.com/rainmeter/rainmeter/blob/54df7737c615ebe5a6b1d63e539aec171b54aec7/Library/System.cpp) | layered 최상위 창, Show Desktop 상태에 따른 Z 순서 관리, 감시용 보조 창 | 실용적인 바탕화면 위젯의 다른 접근. 단순 SetParent 예제가 아님. 상태 관리 전체를 도입하지 않고 이번에는 Progman 안의 자식 창 방식을 검증 |

공개 코드는 비교 조사에 사용했고 소스를 복사해 포함하지 않았다. Win32 API 계약과 기존 Tauri API를 사용해 독립적으로 구현했다.

## 선택한 방법

**호환 OS 선언을 갖춘 실제 Tauri WebView를 독립적인 불투명 layered 자식 창으로 최신 Progman에 연결한다.** 일반 창의 HTML 표시·버튼·Rust IPC를 먼저 확인한 뒤 같은 HWND를 부착한다.

- `POMOTROID_DESKTOP_PROBE=1`인 Windows 디버그 빌드에만 Common Controls v6를 유지한 supportedOS 매니페스트를 적용한다. 일반 개발·릴리스 빌드는 변경하지 않는다. 실행 파일을 재추출해 실제 선언을 확인했다.
- 기존 타이머·설정·통계와 별도 세대 라벨 `desktop-probe-*`인 WebView다. 통계·설정 스토어와 DB를 읽거나 쓰지 않는다. 흰 배경에 상태·클릭 횟수·조작 버튼 3개만 둔다.
- WebView 생성은 별도 스레드, 네이티브 조작은 Tauri UI 스레드에서 수행해 동기 콜백의 WebView2 생성 교착을 피한다.
- `WS_CHILD`와 layered 표면을 SetParent 전에 준비하고 alpha 255를 읽어 확인한다. 부모 DPI context가 다르면 중단한다.
- 아이콘을 포함한 Progman의 **작은 자식 창**을 아이콘 뷰 앞에 둔다. 일반 앱의 최상위 창보다 아래인 구조다. 항상 위에 표시·클릭 통과·전면 입력 창·전역 입력 후킹은 사용하지 않는다. 위젯 사각형 안에서 겹치는 아이콘은 가려지지만 사각형 밖은 덮지 않는다.
- 자식 창·layered 플래그를 Tao의 스타일 재생성으로부터 보호한다. 타이머의 기존 불투명도 처리에는 손대지 않는다.
- API 실패 시 원래 style/exstyle과 부모 없는 상태로 되돌린다. 복구 실패 시 일반 WebView 재생성을 시도한다.
- Explorer 재시작으로 HWND가 없어지면 WebView를 재생성하고 ready IPC 후 재부착한다. 새 호스트가 나타나지 않으면 제한 시간 후 일반 창을 유지한다. 일반 창은 실패 시 복구용이며 제품 대체안이 아니다.

## 이번 검증 기록

- `npm run check`: 오류 0·경고 0. 기존 Rust 테스트 95개 통과, Clippy 통과. 후속 코드 수정 시 관련 검사를 다시 수행한다.
- 실제 Tauri WebView 일반 창에서 HTML 글자·버튼을 화면으로 확인하고 Click test로 0→1, Rust IPC 수신을 확인했다.
- 부착 시 alpha 255·부모 일치·오류 없음을 로그로 확인했다. **실제 표시 성공의 최종 판정은 아니다.**
- 자동화 도구는 부착 후 자식 창을 usable app window로 취급하지 않아 Win+D 후 실제 표시와 클릭은 사용자에게 한 단계씩 요청한다.
- 현재 환경은 모니터 1대 2560×1440, DPI 96. 다중 모니터·DPI 변경·다른 Windows 빌드는 미검증이다.
- 타이머·불투명도·호버·기존 데이터 분리를 유지한다. 통계 연결·push는 하지 않았다.

### Explorer 재시작의 추가 결함과 검증

첫 재시작에서 타이머는 살아 있었으나 위젯은 자동 복구되지 않았다. 진단을 추가한 재시험에서는 네이티브 HWND 손실 후 Tauri의 이전 라벨 등록이 남아 있었고, `WebviewWindow::destroy()` 요청 후에도 15초 동안 정리되지 않아 재생성이 막혔다. 이는 실제 로그로 확인한 현상이다. `WM_CLOSE`를 사용자 종료와 구분하는 보완은 했지만 이 재시작에서 해당 메시지가 원인이었다고 확인하지는 못했다.

부착용 네이티브 부모를 한 단계 추가하고 `WM_DESTROY`에서 WebView를 먼저 분리하는 시도도 했다. 같은 프로세스의 숨김 창 파괴 테스트는 통과했으나 실제 Explorer 강제 종료에서는 구제 콜백 없이 자식 HWND가 사라져 실패했다. 해당 보조 부모 코드는 최종 경로에서 제외하고 실패 실험을 target/desktop-research에 보관했다. 별도 숨김 부모 프로세스 종료 비교에서도 WS_CHILD와 WS_POPUP 모두 사라졌다. 따라서 원래 스타일을 유지하는 것만으로 생존을 보장할 수 없었다.

현재 복구는 네이티브 HWND 손실 시 공개 `Webview::close()`로 이전 WebView 리소스를 닫고, `desktop-probe-1`, `desktop-probe-2`처럼 새 세대 라벨로 실제 WebView를 생성한 뒤 ready IPC를 거쳐 재부착한다. 실패한 고정 라벨 재사용을 반복하지 않는다. 이전 Tauri 창 메타데이터가 남을 수 있다는 제약은 로그의 retired_count와 함께 기록하며, 장시간 반복 시 자원 정리까지 완전히 검증한 것으로 간주하지 않는다. 정상 종료 때는 개발 시험용 감독 코드가 앱 종료를 요청해 남은 시험 창 등록 때문에 백그라운드 프로세스가 남지 않게 한다.

새 세대 방식의 실제 재시작 결과: Explorer PID 64608 → 68772, 타이머 PID 70560 유지. 12:26:05에 이전 `desktop-probe-1`의 `Webview::close()`가 `Ok(())`, retired_count=1을 기록했고, 생성 #2의 새 HWND에서 ready IPC와 새 Progman 부착·자기 WebView 입력 위치 판정을 확인했다. 이후 사용자에게 복구된 창의 실제 표시와 클릭 횟수 증가를 확인받았다. 최종 경로의 Rust 테스트는 95개, Clippy 통과이며 제거한 보조 부모 방식의 단위 테스트를 현재 복구 성공 근거로 사용하지 않는다.

후속 사용자 확인: Win+D 후 흰 WebView와 글자·버튼이 선명하게 표시됐고, 부착 상태에서 `Click test` 횟수가 증가했다. 일반 앱 창이 위젯과 겹치면 일반 앱이 위젯을 가렸다. Explorer 재시작 후 Win+D에서 창과 `생성 #2`가 다시 보이고 클릭 횟수가 증가함을 확인받았다. 위젯 밖 바탕화면 아이콘 선택도 정상이라고 확인받았다.

## 현재 판정

**현재 Windows 빌드·단일 모니터·100% 배율에서 최소 Tauri WebView의 바탕화면 부착 기본 동작 검증을 통과했다.** 일반 통계 창을 제품 대안으로 추가하지 않았고 기존 통계·잔디 연결은 아직 시작하지 않았다.

| 항목 | 근거 | 결과 |
| --- | --- | --- |
| 실제 Tauri WebView 일반 창 표시·클릭 | 자동화 화면과 Rust IPC | 통과 |
| 바탕화면에서 흰 창·글자·버튼 표시 | 사용자 확인 | 통과 |
| Win+D 뒤 유지 | 사용자 확인 | 통과 |
| 부착 상태의 실제 클릭 | 사용자 확인 및 Rust IPC | 통과 |
| 일반 앱 아래 배치 | 사용자 확인 | 통과 |
| 위젯 밖 바탕화면 아이콘 선택 | 사용자 확인 | 통과 |
| Explorer 재시작 뒤 표시·클릭 복구 | 새 세대 로그와 사용자 확인 | 통과 |
| 다중 모니터·DPI 전환·다른 Windows 빌드 | 재현 환경 없음 | 미검증 |
| 반복 Explorer 재시작의 잔존 창 메타데이터·장시간 자원 정리 | 이전 창 등록 잔존이 실제 관찰됨 | 배포 전 해결·검증 필요 |

이 결과는 무제한 반복·장시간 실행의 배포 안정성을 보장하지 않는다. 타이머·불투명도·호버 코드는 유지했고 원본 데이터에 접근하지 않았다. ④ 변경은 미커밋이며 push하지 않았다.
