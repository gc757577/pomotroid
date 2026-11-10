# ClearPomo 아이콘

정식 표시 이름은 **ClearPomo**, 한국어 소개 표기는 **클리어포모**다.

- 원본: 사용자가 제공한 `반시계 방향 카운트다운 포모도로 아이콘.png` (원본 파일 수정 없음).
- 최종 마스터: `clearpomo.svg`. 크림색 둥근 사각형, 살구/코럴 링, 왼쪽 위 옅은 조각과 두 틈을 유지한 벡터 정리본이다.
- 투명 여백은 각 변의 4%, 타일은 캔버스의 92%. 바깥 그림자·회색 배경·잔 픽셀이 없다.
- `clearpomo.png`: 1024×1024 RGBA. Tauri CLI로 SVG에서 생성했다.
- 앱 PNG는 16/24/32/48/64/128/256/512px. ICO에는 16/24/32/48/64/256px 이미지가 들어 있다. 기존 ICNS·Windows 로고 PNG도 같은 마스터로 갱신했다.
- 정보 화면/웹뷰 아이콘은 `static/app-icon.png`, Windows 앱·설치/제거 아이콘은 `src-tauri/icons/icon.ico`다.

처음에는 내장 imagegen 편집 모드로 원본의 외부 회색 제거와 투명화를 시도했다. 편집 지시는 원본 구도와 색 유지, 왼쪽 위 틈/옅은 조각 유지, 4% 투명 여백, 작은 아이콘용 선명한 윤곽이었다. 생성 결과의 가장자리 잔 픽셀을 확인해 최종 자산은 위 SVG의 둥근 사각형/원호로 정리했다. imagegen 중간 결과는 앱에서 사용하지 않는다. 최종 PNG/ICO에 Python 래스터 편집을 사용하지 않았다.

## 다시 생성

프로젝트 루트에서 실행한다. 생성 결과 중 기존 데스크톱 자산만 `src-tauri/icons`에 복사하고, 256px PNG를 `static/app-icon.png`로 복사한다. 모바일용 생성물은 빌드 임시 폴더에만 둔다.

```powershell
npx tauri icon assets/branding/clearpomo.svg --output src-tauri/target/clearpomo-icons
npx tauri icon assets/branding/clearpomo.svg --output src-tauri/target/clearpomo-icons/custom --png 16 --png 24 --png 48 --png 1024
```

트레이는 정적 아이콘 파일을 그대로 쓰지 않는다. `tray::render_tray_icon_rgba`가 같은 크림 타일과 왼쪽 위 조각을 그리고, 기존 라운드 테마 색상·남은/경과 진행률·일시정지 막대를 유지한다. 카운트다운에서는 활성 원호의 끝이 반시계 방향으로 줄어든다. 기본 테마가 아닌 사용자 테마에서는 진행 원호의 색이 해당 테마 색상을 따른다.

PC UI를 조작하지 않는 실제 트레이 함수 미리보기:

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --locked --offline --example tray-preview -- src-tauri/target/branding-preview
```
