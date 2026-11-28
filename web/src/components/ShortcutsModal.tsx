import { createSignal, For, type JSX } from 'solid-js';
import { locale, setLocale, t, toggleLocale } from '../i18n';
import { Modal } from './bits';

export const COLORBLIND_STORAGE_KEY = 'isle.colorblind';

/** Safely reads persisted colorblind mode (`'isle.colorblind'`) without BoardRenderer coupling. */
export function readColorblindPref(): boolean {
  try {
    if (typeof window !== 'undefined') {
      const raw = window.localStorage.getItem(COLORBLIND_STORAGE_KEY);
      return raw === '1' || raw === 'true';
    }
  } catch {
    // Sandboxed iframes (`dist-embed`) throw SecurityError on localStorage access.
  }
  return false;
}

/** Safely persists colorblind mode (`'isle.colorblind'`) to `localStorage`. */
export function writeColorblindPref(enabled: boolean): void {
  try {
    if (typeof window !== 'undefined') {
      window.localStorage.setItem(COLORBLIND_STORAGE_KEY, enabled ? '1' : '0');
    }
  } catch {
    // Ignore blocked storage.
  }
}

/** Compact language switch button matching `ThemeToggle` / `SoundToggle` in topbars. */
export function LangToggle(props: { class?: string }) {
  return (
    <button
      type="button"
      class={props.class ?? 'ghost'}
      data-testid="lang-toggle"
      aria-label={t('切换到英文界面 (Switch to English)', 'Switch to Chinese (切换到中文界面)')}
      title={t('切换语言 / Switch Language', 'Switch Language / 切换语言')}
      onClick={() => toggleLocale()}
    >
      {locale() === 'zh' ? 'EN' : '中文'}
    </button>
  );
}

export interface ShortcutsModalProps {
  colorblind?: boolean;
  onToggleColorblind?: () => void;
  onClose: () => void;
}

interface ShortcutItem {
  readonly keys: readonly string[];
  readonly sep?: string;
  readonly zh: string;
  readonly en: string;
}

const SHORTCUT_ITEMS: readonly ShortcutItem[] = [
  {
    keys: ['Space'],
    zh: '掷骰子 / 回放播放暂停',
    en: 'Roll Dice / Play-Pause Replay',
  },
  {
    keys: ['E'],
    zh: '结束当前回合',
    en: 'End Turn',
  },
  {
    keys: ['1', '2', '3'],
    sep: '/',
    zh: '切换建造：道路 / 村庄 / 城市',
    en: 'Build Road / Settlement / City',
  },
  {
    keys: ['4'],
    zh: '造船（航海）或造桥（商队）',
    en: 'Build Ship / Bridge',
  },
  {
    keys: ['T'],
    zh: '打开/关闭交易面板',
    en: 'Toggle Trade Panel',
  },
  {
    keys: ['R'],
    zh: '打开规则速查卡',
    en: 'Open Rules Reference',
  },
  {
    keys: ['?'],
    zh: '打开快捷键与无障碍面板',
    en: 'Open Shortcuts & Accessibility',
  },
  {
    keys: ['M'],
    zh: '静音 / 开启桌游音效',
    en: 'Mute / Unmute Sound',
  },
  {
    keys: ['Esc'],
    zh: '取消当前建造模式或关闭弹窗',
    en: 'Cancel Mode / Close Modal',
  },
  {
    keys: ['←', '→'],
    sep: '/',
    zh: '回放模式单步后退 / 前进',
    en: 'Step Replay Back / Forward',
  },
];

const KEYCAP_STYLE: JSX.CSSProperties = {
  display: 'inline-flex',
  'align-items': 'center',
  'justify-content': 'center',
  'min-width': '24px',
  height: '22px',
  padding: '0 6px',
  'border-radius': '5px',
  background: 'rgba(255, 255, 255, 0.1)',
  border: '1px solid rgba(255, 255, 255, 0.22)',
  'box-shadow': 'inset 0 -2px 0 rgba(0, 0, 0, 0.35)',
  'font-family': 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
  'font-size': '11.5px',
  'font-weight': '600',
  color: 'var(--accent)',
  'line-height': '1',
  'white-space': 'nowrap',
};

const SETTING_ROW_STYLE: JSX.CSSProperties = {
  display: 'flex',
  'flex-wrap': 'wrap',
  'align-items': 'center',
  'justify-content': 'space-between',
  gap: '8px 12px',
};

export function ShortcutsModal(props: ShortcutsModalProps) {
  const [fallbackColorblind, setFallbackColorblind] = createSignal(readColorblindPref());

  const isColorblind = () =>
    props.colorblind !== undefined ? props.colorblind : fallbackColorblind();

  const handleSetColorblind = (want: boolean) => {
    if (isColorblind() === want) return;
    if (props.onToggleColorblind) {
      props.onToggleColorblind();
    } else {
      writeColorblindPref(want);
      setFallbackColorblind(want);
    }
  };

  return (
    <Modal
      title={t('快捷键与无障碍设置', 'Shortcuts & Accessibility')}
      onClose={props.onClose}
      wide
    >
      <section data-testid="a11y-settings-section">
        <h4>{t('无障碍与语言', 'Accessibility & Language')}</h4>
        <div class="rules-box" style={{ gap: '10px', padding: '10px 12px' }}>
          <div style={SETTING_ROW_STYLE}>
            <div style={{ display: 'flex', 'flex-direction': 'column', gap: '2px' }}>
              <span style={{ 'font-weight': '600' }}>{t('界面语言', 'Language')}</span>
              <span class="muted small">
                {t('即时切换全局界面语言', 'Instantly switch UI display language')}
              </span>
            </div>
            <div class="seg small" role="group" aria-label={t('界面语言', 'Language')}>
              <button
                type="button"
                data-testid="lang-zh"
                aria-pressed={locale() === 'zh'}
                classList={{ on: locale() === 'zh' }}
                onClick={() => setLocale('zh')}
              >
                中文
              </button>
              <button
                type="button"
                data-testid="lang-en"
                aria-pressed={locale() === 'en'}
                classList={{ on: locale() === 'en' }}
                onClick={() => setLocale('en')}
              >
                English
              </button>
            </div>
          </div>

          <div
            style={{
              ...SETTING_ROW_STYLE,
              'padding-top': '8px',
              'border-top': '1px solid rgba(255, 255, 255, 0.07)',
            }}
          >
            <div style={{ display: 'flex', 'flex-direction': 'column', gap: '2px' }}>
              <span style={{ 'font-weight': '600' }}>
                {t('色盲高对比纹章模式', 'Colorblind Symbols')}
              </span>
              <span class="muted small">
                {t(
                  '为各阵营棋子叠加几何徽记，并在地形格显示高对比文字标',
                  'Adds geometric player crests and high-contrast terrain text labels',
                )}
              </span>
            </div>
            <div
              class="seg small"
              role="group"
              aria-label={t('色盲高对比纹章模式', 'Colorblind Symbols')}
            >
              <button
                type="button"
                data-testid="colorblind-off"
                aria-pressed={!isColorblind()}
                classList={{ on: !isColorblind() }}
                onClick={() => handleSetColorblind(false)}
              >
                {t('关闭', 'Off')}
              </button>
              <button
                type="button"
                data-testid="colorblind-on"
                aria-pressed={isColorblind()}
                classList={{ on: isColorblind() }}
                onClick={() => handleSetColorblind(true)}
              >
                {t('开启（棋子几何徽记 + 地形文字标）', 'On (Piece Symbols + Hex Labels)')}
              </button>
            </div>
          </div>
        </div>
      </section>

      <section data-testid="keyboard-shortcuts-section">
        <h4>{t('键盘快捷键速查', 'Keyboard Shortcuts')}</h4>
        <div
          class="rules-box"
          style={{
            display: 'grid',
            'grid-template-columns': 'repeat(auto-fit, minmax(220px, 1fr))',
            gap: '7px 12px',
            padding: '10px 12px',
          }}
        >
          <For each={SHORTCUT_ITEMS}>
            {(item) => (
              <div
                style={{
                  display: 'flex',
                  'align-items': 'center',
                  gap: '8px',
                  padding: '4px 6px',
                  'border-radius': '6px',
                  background: 'rgba(255, 255, 255, 0.03)',
                }}
              >
                <span
                  style={{
                    display: 'inline-flex',
                    'align-items': 'center',
                    gap: '3px',
                    flex: 'none',
                  }}
                >
                  <For each={item.keys}>
                    {(k, idx) => (
                      <>
                        {idx() > 0 && item.sep ? (
                          <span class="muted small" style={{ 'font-size': '11px' }}>
                            {item.sep}
                          </span>
                        ) : null}
                        <kbd style={KEYCAP_STYLE}>{k}</kbd>
                      </>
                    )}
                  </For>
                </span>
                <span class="small" style={{ color: 'var(--text)', 'line-height': '1.3' }}>
                  {t(item.zh, item.en)}
                </span>
              </div>
            )}
          </For>
        </div>
      </section>

      <footer>
        <span class="muted small">
          {t(
            '提示：对局中在非输入框状态下可直接触发快捷键',
            'Tip: Shortcuts are active whenever focus is outside text inputs',
          )}
        </span>
        <button type="button" class="primary" onClick={props.onClose}>
          {t('关闭', 'Close')}
        </button>
      </footer>
    </Modal>
  );
}
