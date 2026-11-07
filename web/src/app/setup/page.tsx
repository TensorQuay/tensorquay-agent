import { AuthForm } from "../../auth/AuthForm";

export default function Setup() {
  return (
    <main id="main" className="login-page">
      <AuthForm mode="setup" />
    </main>
  );
}
