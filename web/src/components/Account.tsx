import { createSignal, Show } from 'solid-js';
import { errorText } from '../game/text';
import { account, accountsEnabled, checkRegistration, login, logout, logoutEverywhere, register } from '../net/account';

/** Login / registration form, or the signed-in account with a logout button. */
export function AccountPanel() {
  return (
    <Show when={accountsEnabled()}>
      <Show when={account()} fallback={<SignIn />}>
        {(a) => <SignedIn username={a().username} />}
      </Show>
    </Show>
  );
}

function SignedIn(props: { username: string }) {
  const [error, setError] = createSignal<string | null>(null);
  return (
    <div class="account-bar">
      <span class="grow">
        已登录 <b class="account-name">{props.username}</b>
        <Verified />
      </span>
      <button class="small" onClick={() => void logout()}>
        退出登录
      </button>
      <button
        class="small ghost"
        title="让所有设备上的登录都失效"
        onClick={async () => setError(await logoutEverywhere())}
      >
        退出所有设备
      </button>
      <Show when={error()}>
        <p class="form-error">{errorText(error()!)}</p>
      </Show>
    </div>
  );
}

function SignIn() {
  const [mode, setMode] = createSignal<'login' | 'register'>('login');
  const [username, setUsername] = createSignal('');
  const [password, setPassword] = createSignal('');
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  const submit = async (e: Event) => {
    e.preventDefault();
    if (busy()) return;
    const registering = mode() === 'register';
    const local = registering ? checkRegistration(username(), password()) : username().trim() && password() ? null : 'MissingCredentials';
    if (local) {
      setError(local);
      return;
    }
    setBusy(true);
    setError(null);
    const err = await (registering ? register : login)(username(), password());
    setBusy(false);
    if (err) setError(err);
    else setPassword('');
  };

  return (
    <details class="account">
      <summary>登录 / 注册账号（可选）</summary>
      <form class="account-form" onSubmit={submit}>
        <p class="muted small">登录后用户名就是你的昵称，在任何设备打开邀请链接都能回到自己的座位。不登录也能直接玩。</p>
        <div class="seg small">
          <button type="button" classList={{ on: mode() === 'login' }} onClick={() => (setMode('login'), setError(null))}>
            登录
          </button>
          <button type="button" classList={{ on: mode() === 'register' }} onClick={() => (setMode('register'), setError(null))}>
            注册
          </button>
        </div>
        <input
          name="username"
          value={username()}
          maxLength={32}
          autocomplete="username"
          autocapitalize="off"
          spellcheck={false}
          placeholder="用户名"
          aria-label="用户名"
          onInput={(e) => setUsername(e.currentTarget.value)}
        />
        <input
          name="password"
          type="password"
          value={password()}
          autocomplete={mode() === 'login' ? 'current-password' : 'new-password'}
          placeholder={mode() === 'login' ? '密码' : '密码（至少 12 个字符）'}
          aria-label="密码"
          onInput={(e) => setPassword(e.currentTarget.value)}
        />
        <Show when={mode() === 'register'}>
          <p class="muted small">用户名只能用字母、数字、_ 和 -（不区分大小写），最长 32 位。</p>
        </Show>
        <Show when={error()}>
          <p class="form-error" role="alert">
            {errorText(error()!)}
          </p>
        </Show>
        <button class="primary" type="submit" disabled={busy()}>
          {busy() ? '请稍候…' : mode() === 'login' ? '登录' : '注册并登录'}
        </button>
      </form>
    </details>
  );
}

/** Mark for players signed in with an account. */
export function Verified() {
  return (
    <span class="verified" title="已登录账号" aria-label="已登录账号">
      ✓
    </span>
  );
}
