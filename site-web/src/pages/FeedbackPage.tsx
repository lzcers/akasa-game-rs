import { motion } from "framer-motion";
import { useState, type JSX } from "react";

type FeedbackType = "bug" | "suggestion" | "story" | "other";

type FeedbackApiResponse =
  | {
      success: true;
      data: {
        feedbackId: string;
        accepted: boolean;
      };
    }
  | {
      success: false;
      error?: {
        message?: string;
      };
    };

const feedbackTypes: {
  value: FeedbackType;
  label: string;
  icon: React.ReactNode;
}[] = [
  {
    value: "bug",
    label: "异常记录",
    icon: (
      <svg
        className="w-5 h-5"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
      >
        <path d="M12 9v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
      </svg>
    ),
  },
  {
    value: "suggestion",
    label: "共鸣建议",
    icon: (
      <svg
        className="w-5 h-5"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
      >
        <path d="M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z" />
      </svg>
    ),
  },
  {
    value: "other",
    label: "其他回声",
    icon: (
      <svg
        className="w-5 h-5"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
      >
        <path d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
      </svg>
    ),
  },
];

export default function FeedbackPage() {
  const [type, setType] = useState<FeedbackType>("suggestion");
  const [email, setEmail] = useState("");
  const [content, setContent] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  type SubmitEvent = Parameters<
    NonNullable<JSX.IntrinsicElements["form"]["onSubmit"]>
  >[0];

  const handleSubmit = async (e: SubmitEvent) => {
    e.preventDefault();
    if (isSubmitting) {
      return;
    }

    const trimmedContent = content.trim();
    if (!trimmedContent) {
      setSubmitError("请先写下你的回声。");
      return;
    }

    setIsSubmitting(true);
    setSubmitError(null);

    try {
      const response = await fetch("/api/feedback", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        credentials: "same-origin",
        body: JSON.stringify({
          type,
          email: email.trim() || undefined,
          content: trimmedContent,
          page: window.location.pathname,
          userAgent: window.navigator.userAgent,
        }),
      });

      const isJson = response.headers
        .get("content-type")
        ?.includes("application/json");
      const body = isJson
        ? ((await response.json()) as FeedbackApiResponse)
        : null;

      if (!response.ok || body?.success !== true) {
        setSubmitError(
          body?.success === false && body.error?.message
            ? body.error.message
            : `回声通道返回 ${response.status}`,
        );
        return;
      }

      setSubmitted(true);
    } catch (error) {
      setSubmitError(
        error instanceof Error
          ? error.message
          : "回声通道暂时不可用，请稍后再试。",
      );
    } finally {
      setIsSubmitting(false);
    }
  };

  if (submitted) {
    return (
      <div className="min-h-screen py-24 px-6">
        <div className="max-w-4xl mx-auto">
          <motion.div
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            className="game-card p-8 lg:p-10"
          >
            <div className="grid gap-8 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)] lg:items-center">
              <div className="text-center lg:text-left">
                <div className="w-16 h-16 mx-auto lg:mx-0 mb-6 rounded-full bg-accent/20 flex items-center justify-center">
                  <svg
                    className="w-8 h-8 text-accent"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M5 13l4 4L19 7"
                    />
                  </svg>
                </div>
                <p className="text-xs tracking-[0.32em] text-accent/80 mb-3">
                  回声已收录
                </p>
                <h2 className="font-serif text-2xl lg:text-3xl text-foreground mb-3">
                  你的回声已被收录
                </h2>
                <p className="text-sm text-muted-foreground leading-7">
                  我们会沿着这条记录继续校准回响。
                  你的每一个观察，都会帮助阿卡夏显影出更贴近玩家想象的世界。
                </p>
              </div>

              <div className="game-card p-5 lg:p-6 bg-background/40">
                <p className="text-sm text-foreground/85 leading-7 mb-4">
                  如果你还有更多想法或者建议，可以继续反馈。
                </p>
                <button
                  onClick={() => {
                    setSubmitted(false);
                    setContent("");
                    setSubmitError(null);
                  }}
                  className="w-full sm:w-auto game-btn-secondary px-6 py-3"
                >
                  继续写回声
                </button>
              </div>
            </div>
          </motion.div>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen py-24 px-6">
      <div className="max-w-6xl mx-auto">
        <div className="grid gap-8 xl:grid-cols-[minmax(18rem,0.85fr)_minmax(0,1.15fr)] xl:items-start">
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            className="space-y-5 xl:sticky xl:top-28"
          >
            <div className="text-center xl:text-left">
              <p className="text-xs tracking-[0.32em] text-accent/80 mb-3">
                Email
              </p>
              <h1 className="font-serif text-3xl md:text-4xl text-foreground mb-3">
                回音信箱
              </h1>
              <p className="text-muted-foreground text-sm leading-7 max-w-md mx-auto xl:mx-0">
                你的声音会影响阿游戏的迭代。
              </p>
            </div>

            <div className="game-card p-5">
              <p className="text-sm text-foreground mb-3">
                写入前可以补足这些线索
              </p>
              <div className="space-y-3 text-sm text-muted-foreground">
                <p>
                  欢迎提出你的想法和建议，如愿意收到回信的话，可以下你的邮箱或者联系方式。
                </p>
              </div>
            </div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.2 }}
            >
              <h3 className="font-serif text-lg text-foreground mb-4 text-center xl:text-left">
                常见问题
              </h3>
              <div className="space-y-3">
                {[
                  {
                    q: "本地记录丢失怎么办？",
                    a: "游戏记录保存在当前浏览器中。如果清除了浏览器缓存，记录可能会丢失。建议定期导出重要记录。",
                  },
                ].map((item, i) => (
                  <div key={i} className="game-card p-4">
                    <h4 className="text-sm text-foreground mb-1.5">{item.q}</h4>
                    <p className="text-xs text-muted-foreground leading-relaxed">
                      {item.a}
                    </p>
                  </div>
                ))}
              </div>
            </motion.div>
          </motion.div>

          <motion.form
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.1 }}
            onSubmit={handleSubmit}
            className="game-card p-6 lg:p-8"
          >
            <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(15rem,0.68fr)] lg:items-start">
              <div>
                <div className="mb-6">
                  <label className="block text-sm text-muted-foreground mb-3">
                    回声类型
                  </label>
                  <div className="grid grid-cols-2 gap-2">
                    {feedbackTypes.map((item) => (
                      <button
                        key={item.value}
                        type="button"
                        onClick={() => setType(item.value)}
                        className={`p-3 rounded-xl border text-left transition-all flex items-center gap-3 ${
                          type === item.value
                            ? "border-accent bg-accent/10 text-foreground"
                            : "border-border bg-card/30 text-muted-foreground hover:border-border/80"
                        }`}
                      >
                        <span
                          className={type === item.value ? "text-accent" : ""}
                        >
                          {item.icon}
                        </span>
                        <span className="text-sm">{item.label}</span>
                      </button>
                    ))}
                  </div>
                </div>

                <div className="mb-6">
                  <label
                    htmlFor="content"
                    className="block text-sm text-muted-foreground mb-2"
                  >
                    回声内容
                  </label>
                  <textarea
                    id="content"
                    value={content}
                    onChange={(e) => setContent(e.target.value)}
                    required
                    rows={8}
                    placeholder="写下你看到的问题、希望共鸣出的世界或角色，或某段故事的不协调之处..."
                    className="w-full px-4 py-3 rounded-xl border border-border bg-card/50 text-foreground placeholder:text-muted-foreground/40 focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/20 transition-all resize-none"
                  />
                </div>
              </div>

              <div>
                <div className="mb-6">
                  <label
                    htmlFor="email"
                    className="block text-sm text-muted-foreground mb-2"
                  >
                    联系邮箱/微信{" "}
                    <span className="text-muted-foreground/50">(可选)</span>
                  </label>
                  <input
                    id="email"
                    type="email"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                    placeholder=""
                    className="w-full px-4 py-3 rounded-xl border border-border bg-card/50 text-foreground placeholder:text-muted-foreground/40 focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/20 transition-all"
                  />
                  <p className="mt-2 text-xs text-muted-foreground/60">
                    如需我们回信，请留下邮箱
                  </p>
                </div>

                {submitError ? (
                  <p className="mb-4 rounded-xl border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                    {submitError}
                  </p>
                ) : null}

                <motion.button
                  type="submit"
                  disabled={isSubmitting}
                  whileHover={{ scale: 1.01 }}
                  whileTap={{ scale: 0.99 }}
                  className={`w-full game-btn-primary py-3.5 ${
                    isSubmitting ? "cursor-not-allowed opacity-70" : ""
                  }`}
                >
                  {isSubmitting ? "写入中..." : "送入记录"}
                </motion.button>
              </div>
            </div>
          </motion.form>
        </div>
      </div>
    </div>
  );
}
