import { AuthForm } from "../../auth/AuthForm";

export default function Login() {
  return (
    <main id="main" className="login-page">
      <AuthForm mode="login" />
    </main>
  );
}
