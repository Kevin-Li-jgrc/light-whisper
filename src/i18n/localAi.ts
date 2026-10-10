import i18n from "./index";
import zhUrl from "./localAi.zh.json?url&no-inline";
import enUrl from "./localAi.en.json?url&no-inline";

let loading: Promise<void> | undefined;
// 仅设置页需要这些文案；通过安装包内的静态资源加载，避免增加启动脚本。
export function loadLocalAiTranslations(): Promise<void> {
  loading ??= Promise.all(([ ["zh", zhUrl], ["en", enUrl] ] as const).map(async ([language, url]) => {
    const response = await fetch(url);
    if (!response.ok) throw new Error("Local AI translations unavailable");
    const localAi: unknown = await response.json();
    i18n.addResourceBundle(language, "translation", { localAi }, true, true);
  })).then(() => undefined).catch((error) => { loading = undefined; throw error; });
  return loading;
}
