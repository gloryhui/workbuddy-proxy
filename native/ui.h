#ifndef WB_UI_H
#define WB_UI_H
extern void wb_frontend_ready(void);
extern void wb_frontend_closing(void);
extern char *wb_value(const char *name);
extern char *wb_command(const char *name);
extern void wb_free(char *value);
extern void wb_select(int index);
extern void wb_use_language(int zh);
extern int wb_ui_zh(void);
extern char *wb_language(const char *language);
extern void wb_models_load(void);
extern int wb_models_count(void);
extern char *wb_model(int row,int column);
extern void wb_model_set(int row,int column,const char *text);
extern int wb_model_add(void);
extern void wb_model_remove(int row);
extern char *wb_save_table(const char *name,const char *key,const char *url,const char *effort);
extern char *wb_fetch(const char *url,const char *key);
extern char *wb_snapshot(void);
extern char *wb_message(const char *text);
extern char *wb_check_update(int force);
enum { T_SUBTITLE,T_PROVIDERS,T_NAME,T_URL,T_KEY,T_MODELS,T_DISPLAY,T_ID,T_FETCH,T_ADD_MODEL,T_REMOVE_MODEL,T_EFFORT,T_SAVE,T_TEST,T_COPY,T_SYNC,T_RESTORE,T_ADD,T_DUPLICATE,T_DELETE,T_INFO,T_WORKING,T_COPIED,T_CLIPBOARD,T_SETTINGS,T_QUIT,T_DEFAULT,T_OFF,T_LOW,T_HIGH,T_MAX,T_KEEP,T_READY,T_FETCH_HINT,T_MODEL_HINT,T_KEY_HINT,T_EVENTS,T_DELETE_QUESTION,T_DISCARD,T_LIMIT,T_LANGUAGE,T_TEST_HINT,T_EMPTY_TITLE,T_EMPTY_HINT,T_ADD_PROVIDER,T_COUNT };
static const char *tr(int id){
    static const char *zh[T_COUNT]={"轻量转发 · 多模型管理","供应商","供应商名称","API 地址","API Key","模型列表","显示名","模型 ID","获取模型","添加","移除","思考强度","保存","测试连接","复制配置","同步 WorkBuddy","恢复配置","新增","复制","删除","运行信息与日志","正在处理…","配置已复制","剪贴板忙，请重试","设置","退出","跟随 WorkBuddy","关闭思考","低","高","最高","保留各模型设置","就绪","使用当前地址和 Key 获取可用模型","双击编辑；显示名默认使用模型 ID","请输入 API Key","本次界面操作","删除此供应商？保存的 Key 和模型将一并移除。","有未保存的修改。是否放弃并切换供应商？","最多支持 256 个模型","语言","使用已保存的首个模型测试连接","尚未添加供应商","添加供应商后，配置 API 地址、Key 和模型。","添加供应商"};
    static const char *en[T_COUNT]={"Lightweight relay · Multiple models","Providers","Provider name","API URL","API Key","Models","Display name","Model ID","Fetch models","Add","Remove","Thinking effort","Save","Test connection","Copy","Sync WorkBuddy","Restore","Add","Copy","Del","System & logs","Working…","Configuration copied","Clipboard busy; retry","Settings","Quit","Follow WorkBuddy","Thinking off","Low","High","Max","Keep per-model settings","Ready","Fetch available models using the current URL and key","Double-click to edit; names default to the model ID","Enter API Key","This window's actions","Delete this provider, including its saved key and models?","Unsaved changes. Discard them and switch provider?","At most 256 models","Language","Test the connection using its first saved model","No providers yet","Add a provider to configure its API URL, key and models.","Add provider"};
    return id>=0&&id<T_COUNT?(wb_ui_zh()?zh[id]:en[id]):"";
}
static const char *efforts[]={"default","none","low","high","max","keep"};
#endif
