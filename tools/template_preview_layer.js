/* ═══════════════ TEMPLATE PREVIEW LAYER ═══════════════
   `assets/report-template.html` 是生成器的输入：占位符由 Rust 替换、
   <!-- INJECT_* --> 槽位由 Rust 填充。所以直接打开这个文件只会看到一堆
   未替换的占位符和空洞 —— 想调样式时根本看不出效果。

   这里在「检测到确实还有未替换的占位符」时，填入一整套示例数据，让模板
   自身就是一份可预览的完整版式（改样式不用跑 uzi）。
   真实报告里占位符已被替换 → 本段整体 return，零 DOM 改动、零副作用。

   注意 1：内联脚本自身的源码也是 documentElement.innerHTML 的一部分，
   所以检测只扫「会真正渲染的文本节点和属性」，并跳过 SCRIPT/STYLE。

   注意 2：花括号一律用 fromCharCode 拼，本文件里绝不出现成对花括号的字面量。
   生成器是对整份文件做**全局文本替换**的，连 JS 注释里的示例都照替 ——
   注释里写一个 NAME 占位符的样例，报告里就会变成股票名，把注释改成语义错误。
   （实测：曾把注释里的示例替换成了 "Bitcoin"，是真实的踩坑记录。）        */
(function () {
  var LB = String.fromCharCode(123, 123);   // 左花括号 ×2
  var RB = String.fromCharCode(125, 125);   // 右花括号 ×2
  // 必须有捕获组，且捕获组只能包住花括号「内部」的名字 —— 回调第 2 个参数才是 token 名。
  // 若把两侧花括号也一起包进捕获组，拿到的是「带花括号的整串」，查 D 表必然落空、
  // 占位符原样留下；若完全不加捕获组，拿到的则是匹配偏移量。两种错法症状一模一样。
  var TOKEN = new RegExp(LB + '([A-Z_0-9]+)' + RB, 'g');

  function hasToken(root) {
    var attrs = root.querySelectorAll('*');
    for (var i = 0; i < attrs.length; i++) {
      var el = attrs[i];
      if (el.tagName === 'SCRIPT' || el.tagName === 'STYLE') continue;
      for (var j = 0; j < el.attributes.length; j++) {
        if (el.attributes[j].value.indexOf(LB) >= 0) return true;
      }
    }
    var w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, null, false);
    while (w.nextNode()) {
      var n = w.currentNode;
      var p = n.parentElement;
      if (p && (p.tagName === 'SCRIPT' || p.tagName === 'STYLE')) continue;
      if (n.nodeValue.indexOf(LB) >= 0) return true;
    }
    return false;
  }
  if (!hasToken(document.documentElement)) return;   // 真实报告：什么都不做

  /* ── 1 · 示例标量 ───────────────────────────────────────── */
  var D = {
    NAME: '贵州茅台', TICKER: '600519.SH', INDUSTRY: '白酒 · 食品饮料',
    ONE_LINER: '高端白酒龙头 · 品牌护城河极深 · 预收款与现金流稳定',
    CURRENCY: '¥', PRICE: '1,586.00', CHANGE_PCT: '+1.24%', CHANGE_DIR: 'up',
    MCAP: '1.99万亿', PE: '22.4', PB: '7.8',
    OVERALL_SCORE_INT: '72',
    VERDICT_LABEL: '中性偏多 · 5 派看多 · 基本面 74.2 · 共识 56.8',
    CORE_CONCLUSION: '贵州茅台 72 分 · 中性偏多。品牌壁垒与渠道掌控力仍是全市场最强的一档，'
      + '预收款/现金流质量支撑估值下沿；压制项来自白酒行业需求增速放缓与批价波动。'
      + '66 位评委里 31 人看多、22 人看空，分歧主要集中在「增速能否回到双位数」。',
    MARKET_STATUS: '已收盘', MARKET_STATUS_CLASS: 'closed',
    DATA_FETCHED_AT: '2026-09-13 18:40', GENERATED_AT: '2026-09-13 18:42',
    PLUGIN_VERSION: '3.9.4',
    TRAP_COLOR: 'safe', TRAP_EMOJI: '🟢', TRAP_LEVEL: '安全',
    TRAP_RECOMMENDATION: '数据正常，未发现异常推广痕迹',
    DP_TREND: '多头排列 · MA20 上穿 MA60', DP_PRICE: '距 5 年高点 -18%',
    DP_VOLUME: '5 日均量 +12%', DP_CHIPS: '主力净流入 3.2 亿',
    INTEL_NEWS: '中秋动销数据好于预期，批价企稳在 2,650 元附近',
    INTEL_RISKS: '白酒行业整体需求增速放缓 · 库存周期仍在去化',
    INTEL_CATALYSTS: '三季报预告 · 直销渠道占比继续提升',
    BP_ENTRY: '¥1,520', BP_POSITION: '3 成', BP_STOP: '¥1,380', BP_TARGET: '¥1,860',
    PUNCHLINE: '💥 护城河仍在，但增速换挡 —— 适合「拿得住」的钱，不适合追高。',
    BULL_ID: 'buffett', BULL_NAME: '巴菲特', BULL_SCORE: '86',
    BULL_TAG: '品牌垄断 + 定价权', BULL_SIGNAL_CN: '买入',
    BEAR_ID: 'mao_lb', BEAR_NAME: '毛老板', BEAR_SCORE: '1',
    BEAR_TAG: '需求见顶 · 估值透支', BEAR_SIGNAL_CN: '回避',
    TOTAL_COUNT: '66', BULL_COUNT: '31', BEAR_COUNT: '22', NEUT_COUNT: '13',
    CONSENSUS_PCT: '57',
    ZONE_VALUE_PRICE: '¥1,380', ZONE_VALUE_RATIONALE: '股息率 3.2% · PE 回落至 19x 的估值底',
    ZONE_GROWTH_PRICE: '¥1,620', ZONE_GROWTH_RATIONALE: '增速回到 12% 所需的估值切换位',
    ZONE_TECH_PRICE: '¥1,540', ZONE_TECH_RATIONALE: 'MA60 支撑 · 前高回踩确认区',
    ZONE_YOUZI_PRICE: '¥1,700', ZONE_YOUZI_RATIONALE: '放量突破前高后的情绪加速位'
  };
  D.ASSET_CLASS_LABEL = 'EQUITY';
  D.METRIC_1_LABEL = 'PE';
  D.METRIC_1 = '22.4';
  D.METRIC_2_LABEL = 'PB';
  D.METRIC_2 = '7.8';
  D.CORE_LABEL = '核心结论';
  D.FIN_CATEGORY = '💰 财务面 · FUNDAMENTALS';
  D.IND_CATEGORY = '🏭 行业面 · INDUSTRY CHAIN';
  D.CO_CATEGORY = '🏢 公司面 · COMPANY';
  D.ENV_CATEGORY = '🌍 环境面 · ENVIRONMENT';
  D.SAFETY_CATEGORY = '🛡️ 安全面 · SAFETY & SENTIMENT';
  D.MODEL_SECTION_LABEL = '机构级估值建模';
  D.ZONES_TITLE = '四派系买入区间';
  D.ZONE_VALUE_LABEL = 'VALUE 价值派';
  D.ZONE_GROWTH_LABEL = 'GROWTH 成长派';
  D.ZONE_TECH_LABEL = 'TECH 技术派';
  D.ZONE_YOUZI_LABEL = 'YOUZI 游资派';

  /* ── 2 · 占位符替换（文本节点 + 属性） ─────────────────── */
  function sub(s) {
    return s.replace(TOKEN, function (m, key) {
      return Object.prototype.hasOwnProperty.call(D, key) ? D[key] : m;
    });
  }
  var all = document.querySelectorAll('*');
  for (var i = 0; i < all.length; i++) {
    var el = all[i];
    if (el.tagName === 'SCRIPT' || el.tagName === 'STYLE') continue;
    for (var j = 0; j < el.attributes.length; j++) {
      var a = el.attributes[j];
      if (a.value.indexOf(LB) >= 0) el.setAttribute(a.name, sub(a.value));
    }
  }
  var tw = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null, false);
  var texts = [];
  while (tw.nextNode()) {
    var node = tw.currentNode, par = node.parentElement;
    if (par && (par.tagName === 'SCRIPT' || par.tagName === 'STYLE')) continue;
    if (node.nodeValue.indexOf(LB) >= 0) texts.push(node);
  }
  texts.forEach(function (n) { n.nodeValue = sub(n.nodeValue); });

  /* ── 3 · 槽位填充 ───────────────────────────────────────── */
  function slot(sel, html, idx) {
    var nodes = document.querySelectorAll(sel);
    var node = nodes[idx || 0];
    if (node) node.innerHTML = html;
    return !!node;
  }
  function afterComment(marker, html) {
    var w = document.createTreeWalker(document.body, NodeFilter.SHOW_COMMENT, null, false);
    while (w.nextNode()) {
      var c = w.currentNode;
      if (c.nodeValue.indexOf(marker) < 0) continue;
      var box = document.createElement('div');
      box.innerHTML = html;
      var frag = document.createDocumentFragment();
      while (box.firstChild) frag.appendChild(box.firstChild);
      c.parentNode.insertBefore(frag, c.nextSibling);
      return true;
    }
    return false;
  }

  var esc = function (s) {
    return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  };

  /* 3.1 · 风格识别 chip */
  afterComment('INJECT_STYLE_CHIP',
    '<div class="style-chip-wrap"><span class="icon">🎯</span>'
    + '<span class="label">本股识别为</span><span class="value">质量成长</span>'
    + '<span class="hint">高 ROE + 稳定现金流 · 价值/成长双派加权</span>'
    + '<span class="compare">fund 74.2→<strong>74.2</strong> · panel 52.8→<strong>56.8</strong>'
    + ' <span class="delta-up">(+4.0)</span></span></div>');

  /* 3.2 · 评委席（66 席 · 按流派分列） */
  // 与 crates/uzi-investors/src/data/investors.json 对齐的真实 66 人名单
  // 分组: A:6 B:9 C:7 D:4 E:7 F:24 G:4 H:4 I:1
  var ROSTER = [
    "buffett|巴菲特|A", "graham|格雷厄姆|A", "fisher|费雪|A",
    "munger|芒格|A", "templeton|邓普顿|A", "klarman|卡拉曼|A",
    "lynch|彼得·林奇|B", "oneill|欧奈尔|B", "thiel|彼得·蒂尔|B",
    "wood|木头姐|B", "andreessen|马克·安德森|B", "gurley|比尔·格利|B",
    "naval|纳瓦尔|B", "gerstner|布拉德·格斯特纳|B", "chamath|查马斯|B",
    "soros|索罗斯|C", "dalio|达里奥|C", "marks|霍华德·马克斯|C",
    "druck|德鲁肯米勒|C", "robertson|罗伯逊|C", "burry|迈克尔·伯利|C",
    "chanos|吉姆·查诺斯|C", "livermore|利弗莫尔|D", "minervini|米内尔维尼|D",
    "darvas|达瓦斯|D", "gann|江恩|D", "duan|段永平|E",
    "zhangkun|张坤|E", "zhushaoxing|朱少醒|E", "xiezhiyu|谢治宇|E",
    "fengliu|冯柳|E", "dengxiaofeng|邓晓峰|E", "zhang_lei|张磊|E",
    "zhang_mz|章盟主|F", "sun_ge|孙哥|F", "zhao_lg|赵老哥|F",
    "fs_wyj|佛山无影脚|F", "yangjia|炒股养家|F", "chen_xq|陈小群|F",
    "hu_jl|呼家楼|F", "fang_xx|方新侠|F", "zuoshou|作手新一|F",
    "xiao_ey|小鳄鱼|F", "jiao_yy|交易猿|F", "mao_lb|毛老板|F",
    "xiao_xian|消闲派|F", "lasa|拉萨天团|F", "chengdu|成都帮|F",
    "sunan|苏南帮|F", "ningbo_st|宁波桑田路|F", "liuyi_zl|六一中路|F",
    "liu_sh|流沙河|F", "gu_bl|古北路|F", "bj_cj|北京炒家|F",
    "wang_zr|瑞鹤仙|F", "xin_dd|鑫多多|F", "ghzw|股海贼王|F",
    "simons|西蒙斯|G", "thorp|索普|G", "shaw|大卫·肖|G",
    "asness|克利夫·阿斯尼斯|G", "jensen_huang|黄仁勋|H", "musk|马斯克|H",
    "altman|山姆·奥特曼|H", "saylor|迈克尔·塞勒|H", "serenity|Serenity|I",
  ];
  /* 示例评分表 —— 66 人逐一给分，而不是伪随机生成。这样：
       ① 席位上的分与群聊里同一个人完全一致；
       ② 全表聚合正好落在 31 看多 / 13 中性 / 22 看空，
          与报告各处引用的「31 人看多 · 22 人看空 · 共识 57%」自洽。
     分档：≥62 看多(bullish) · 38–61 中性(neutral) · <38 看空(bearish)
     按流派核对：A 5/1/0 · B 6/2/1 · C 2/2/3 · D 0/1/3 · E 6/1/0
                 F 6/4/14 · G 2/1/1 · H 3/1/0 · I 1/0/0 */
  var SCORE_STR =
    'buffett:86 munger:81 fisher:74 templeton:68 klarman:66 graham:47 '
    + 'oneill:64 thiel:72 wood:66 andreessen:70 naval:63 gerstner:76 '
    + 'lynch:51 gurley:44 chamath:35 '
    + 'robertson:66 chanos:69 soros:44 dalio:52 marks:31 druck:29 burry:18 '
    + 'livermore:36 minervini:33 darvas:42 gann:25 '
    + 'duan:83 zhangkun:78 zhushaoxing:75 xiezhiyu:71 fengliu:80 dengxiaofeng:73 zhang_lei:58 '
    + 'hu_jl:80 sun_ge:78 lasa:75 fs_wyj:69 liuyi_zl:67 ningbo_st:64 '
    + 'zhang_mz:49 chen_xq:52 jiao_yy:55 xiao_xian:38 '
    + 'zhao_lg:32 yangjia:15 fang_xx:35 zuoshou:22 xiao_ey:18 mao_lb:1 chengdu:21 '
    + 'sunan:29 liu_sh:26 gu_bl:24 bj_cj:34 wang_zr:7 xin_dd:31 ghzw:13 '
    + 'thorp:64 asness:67 simons:55 shaw:10 '
    + 'jensen_huang:72 musk:79 altman:68 saylor:50 '
    + 'serenity:66';
  var SCORES = {};
  SCORE_STR.split(/\s+/).forEach(function (kv) {
    var p = kv.split(':');
    if (p.length === 2 && p[0]) SCORES[p[0]] = +p[1];
  });
  function stanceOf(score) {
    return score >= 62 ? 'bullish' : score >= 38 ? 'neutral' : 'bearish';
  }
  function signalOf(stance) {
    return stance === 'bullish' ? '买入' : stance === 'neutral' ? '观望' : '回避';
  }

  var seats = ROSTER.map(function (row) {
    var parts = row.split('|'), id = parts[0], name = parts[1], group = parts[2];
    var score = SCORES[id];
    var stance = stanceOf(score);
    var signal = signalOf(stance);
    return '<div class="seat ' + stance + '" data-group="' + group + '" data-target="msg-' + id
      + '" title="' + esc(name) + ' · ' + signal + ' · 点击查看完整结论">'
      + '<img alt="" class="seat-avatar" src="avatars/' + id + '.svg"/>'
      + '<div class="seat-name">' + esc(name) + '</div>'
      + '<div class="seat-score">' + score + '</div></div>';
  }).join('');
  slot('#jury-seats', seats);

  /* 3.3 · 群聊（保留系统通知行，追加消息）
     markup 必须与真实报告一致 —— .msg-meta / .msg-group-tag / .msg-signal-dot /
     .msg-score-badge / .msg-confidence / .msg-bubble / .msg-reasoning /
     .msg-comment / .msg-verdict / details.msg-details。
     否则模板里的聊天样式不会被这套示例数据触发，预览就失去了意义。 */
  var chatBox = document.getElementById('chat-messages');
  if (chatBox) {
    var GROUP_LABEL = { A: '价值', B: '成长', C: '宏观', D: '技术', E: '中国',
                        F: '游资', G: '量化', H: '科技', I: '卡位' };
    var NAMES = {};
    ROSTER.forEach(function (r) { var p = r.split('|'); NAMES[p[0]] = p[1]; });

    var MSGS = [
      { id: 'buffett', group: 'A', horizon: '长线',
        hits: ['[权5] 护城河 34/40 强', '[权3] ROE 32.4%，连续 12 年 > 25%', '[权2] 定价权稳定'],
        misses: ['[权4] PE 22.4x 处于 5 年 31% 分位，尚可但不算极端便宜'],
        comment: '我买的是「收费桥」。批价短期波动不重要，重要的是它每年还能提价。'
          + '这种生意我愿意在 20x 附近拿很久。',
        method: ['终身持有好生意，除非基本面永久性恶化',
                 '极度集中，3-5 只占组合 80%+',
                 '品牌或渠道被破坏时才离场'] },
      { id: 'munger', group: 'A', horizon: '长线',
        hits: ['[权5] 商业模式难被颠覆', '[权2] 管理层没撒谎', '[权2] 市场情绪理性'],
        misses: ['[权4] PE 分位 50，等更便宜'],
        comment: '反过来想：这家最可能怎么死？只要品牌和渠道不被破坏，答案就很难成立。'
          + '所以我更关心它会不会做傻事。',
        method: ['反过来想 ——「这家最可能怎么死？」',
                 '极度集中，分散是能力不足的承认',
                 '管理层 incentive 跑偏时警觉'] },
      { id: 'duan', group: 'E', horizon: '长线',
        hits: ['[权5] 生意模式未变', '[权3] 现金流稳定 · 预收款 +18%'],
        misses: ['[权2] 短期批价看不清'],
        comment: '生意模式没变，变的是市场情绪。看不懂短期批价就别做短期，'
          + '按三年维度看，这个位置不贵。',
        method: ['看不懂就不做，做就做三年',
                 '重仓少数看得懂的生意',
                 '生意模式被破坏时退出'] },
      { id: 'fengliu', group: 'E', horizon: '中长线',
        hits: ['[权4] 品牌壁垒强', '[权3] 逆向机会出现'],
        misses: ['[权2] 渠道库存仍需去化'],
        comment: '好生意遇到坏情绪，才是我的位置。现在的分歧不在基本面，'
          + '在大家愿不愿意等。',
        method: ['逆向投资，人弃我取',
                 '集中持有，长期不动',
                 '基本面逻辑被证伪才走'] },
      { id: 'lynch', group: 'B', horizon: '中长线',
        hits: ['[权3] 生意好懂，品牌认知度高'],
        misses: ['[权4] PEG 约 1.9，不在 1-1.5 理想区间'],
        comment: '好公司 ≠ 好价格。我更愿意等一个增速验证后的回调，'
          + '而不是在增速换挡期猜拐点。',
        method: ['PEG 优先，1-1.5 才考虑',
                 '分散持有，以成长股为主',
                 '增速证伪或 PEG 恶化时卖出'] },
      { id: 'marks', group: 'C', horizon: '中长线',
        hits: ['[权3] 库存周期接近尾声'],
        misses: ['[权4] 尚未看到需求拐点'],
        comment: '我们现在处在周期的哪个位置？白酒是典型的库存周期行业，'
          + '去库存尾声往往是最难熬也最值钱的阶段。',
        method: ['先判断周期位置，再谈价格',
                 '逆向布局，越冷越买',
                 '周期见顶信号出现时减仓'] },
      { id: 'simons', group: 'G', horizon: '中长线',
        hits: ['[权4] 质量因子满分'],
        misses: ['[权3] 动量因子中性', '[权3] 估值因子偏贵'],
        comment: '因子层面：质量满分，动量中性，估值偏贵。模型给的是'
          + '「低波动 + 稳定超额」，不是爆发。',
        method: ['纯统计套利，不判断基本面',
                 '全市场分散，单票权重极低',
                 '因子失效或回撤超阈值即调仓'] },
      { id: 'serenity', group: 'I', horizon: '中长线',
        hits: ['[权3] 现金流足以支撑转型尝试'],
        misses: ['[权4] AI 叙事与主业不相关'],
        comment: 'AI 叙事跟它没关系，别硬套。但它的现金流足以支撑任何一次转型尝试，'
          + '这是它的期权价值。',
        method: ['找被忽视的卡位型资产',
                 '小仓位试错，对了再加',
                 '卡位逻辑被证伪时离场'] },
      { id: 'jensen_huang', group: 'H', horizon: '长线',
        hits: ['[权3] 渠道数字化投入领先'],
        misses: ['[权2] 消费品不在我的能力圈'],
        comment: '消费品我不专业，但它对渠道数字化的投入值得关注，'
          + '这决定了它能不能把定价权维持到下一个十年。',
        method: ['只投技术拐点上的赢家',
                 '重仓少数几个赛道',
                 '技术路线被替代时离场'] },
      { id: 'zuoshou', group: 'F', horizon: '短线',
        hits: ['[权2] 量能尚可'],
        misses: ['[权4] 上方套牢盘厚', '[权3] 未放量突破前高'],
        comment: '我做的是弹性，不是价值。放量站上 1,620 我才会进场，否则不参与。',
        method: ['只做弹性和情绪',
                 '快进快出，不留隔夜重仓',
                 '跌破关键位无条件走'] },
      { id: 'burry', group: 'C', horizon: '中长线',
        hits: ['[权4] 需求端在收缩', '[权3] 库存周期下行'],
        misses: ['[权2] 现金流质量仍然优秀'],
        comment: '大家都盯着提价能力，没人看需求端。把增速中枢从 15% 下调到 8% '
          + '再算一遍，估值并不便宜。',
        method: ['找市场共识里的漏洞',
                 '集中做空被高估的标的',
                 '逻辑被数据推翻时平仓'] }
    ];

    var bullet = function (arr) {
      return arr.map(function (s) { return '  • ' + s; }).join('\n');
    };
    var METHOD_KEYS = ['⏱ 时间框架', '💰 仓位风格', '🔄 翻盘条件'];

    chatBox.insertAdjacentHTML('beforeend', MSGS.map(function (m) {
      var score = SCORES[m.id];
      var stance = stanceOf(score);
      var conf = 60 + (score * 7) % 41;            // 稳定落在 60..100
      var name = NAMES[m.id] || m.id;
      var hitLis = m.hits.map(function (h) { return '<li>' + esc(h) + '</li>'; }).join('');
      var missLis = m.misses.map(function (h) { return '<li>' + esc(h) + '</li>'; }).join('');
      var rows = m.method.map(function (r, i) {
        return '<div class="conc-row"><span>' + (METHOD_KEYS[i] || '')
          + '</span><em>' + esc(r) + '</em></div>';
      }).join('');
      var reasoning = '✅ 符合标准：\n' + bullet(m.hits)
        + '\n❌ 未达标准：\n' + bullet(m.misses);

      return '<div class="chat-msg ' + stance + '" data-group="' + m.group
        + '" id="msg-' + m.id + '">'
        + '<img alt="" class="msg-avatar" src="avatars/' + m.id + '.svg"/>'
        + '<div class="msg-body"><div class="msg-meta">'
        + '<span class="msg-name">' + esc(name) + '</span>'
        + '<span class="msg-group-tag">' + m.group + ' · ' + (GROUP_LABEL[m.group] || '') + '</span>'
        + '<span class="msg-signal-dot"></span>'
        + '<span class="msg-score-badge">' + score + '分</span>'
        + '<span class="msg-confidence">conf ' + conf + '</span></div>'
        + '<div class="msg-bubble">'
        + '<div class="msg-reasoning">' + esc(reasoning) + '</div>'
        + '<div class="msg-comment">💬 "' + esc(m.comment) + '"</div>'
        + '<div class="msg-verdict">▸ ' + signalOf(stance) + ' · 周期 ' + m.horizon + '</div>'
        + '<details class="msg-details"><summary>展开完整结论 ▼</summary>'
        + '<div class="conc-content">'
        + '<div class="conc-block"><div class="conc-label">✅ 命中</div><ul>' + hitLis + '</ul></div>'
        + '<div class="conc-block"><div class="conc-label">❌ 未命中</div><ul>' + missLis + '</ul></div>'
        + '<div class="conc-block"><div class="conc-label">🧭 我的方法论</div>' + rows + '</div>'
        + '</div></details>'
        + '</div></div></div>';
    }).join(''));
  }

  /* 3.4 · 三轮辩论 */
  slot('.debate-rounds',
    '<div class="round"><div class="round-label">ROUND 1</div><div class="round-grid">'
    + '<div class="round-bull">看多核心：护城河 34/40 · 定价权 10/10</div>'
    + '<div class="round-vs">VS</div>'
    + '<div class="round-bear">看空核心：增速换挡 · PEG 1.9 未进入林奇理想区间</div></div></div>'
    + '<div class="round"><div class="round-label">ROUND 2</div><div class="round-grid">'
    + '<div class="round-bull">预收款 +18% · 直销占比 46% · 现金流/净利 1.1x</div>'
    + '<div class="round-vs">VS</div>'
    + '<div class="round-bear">行业动销 -4% · 渠道库存 2.1 个月 · 批价同比 -8%</div></div></div>'
    + '<div class="round"><div class="round-label">ROUND 3</div><div class="round-grid">'
    + '<div class="round-bull">综合看，86 分，我的立场不变。</div>'
    + '<div class="round-vs">VS</div>'
    + '<div class="round-bear">综合看，18 分，风险大于收益。</div></div></div>');

  /* 3.5 · 评委汇总观点 */
  afterComment('INJECT_PANEL_INSIGHTS',
    '<div style="font-size:11px;color:#2563eb;letter-spacing:2px;margin-bottom:8px">'
    + '📊 PANEL INSIGHTS · 评委汇总观点 （自动聚合 · agent 未介入）</div>'
    + '<div><strong>66 位评委投票聚合</strong>：31 看多 · 13 中性 · 22 看空。'
    + '共识度 <strong>57%</strong>（neutral 半权计入）。<br><br>'
    + '<strong>按流派分布</strong>：价值派 5✓ / 1○ / 0✗（主流 看多）；'
    + '成长派 6✓ / 2○ / 1✗（主流 看多）；宏观派 2✓ / 2○ / 3✗（主流 看空）；'
    + '技术派 0✓ / 1○ / 3✗（主流 看空）；中国价投 6✓ / 1○ / 0✗（主流 看多）；'
    + 'A 股游资 6✓ / 4○ / 14✗（主流 看空）；量化 2✓ / 1○ / 1✗（主流 中性）；'
    + '科技领袖派 3✓ / 1○ / 0✗（主流 看多）；AI 卡位猎手 1✓ / 0○ / 0✗（主流 看多）。<br><br>'
    + '<strong>分歧焦点</strong>：需求增速能否回到双位数 —— 这是 22 张看空票的主要依据。</div>');

  /* 3.6 · 七大流派各自评分 */
  /* 3.6 · 九大流派各自评分
     「主流」列必须与 SCORE_STR 的分档统计一致（见 3.2 的注释）。 */
  var SCHOOLS = [
    ['📉 经典价值派', 'A', 6, 81, '看多', 'rgba(16,185,129,0.16)', '#065f46'],
    ['🚀 成长派', 'B', 9, 68, '看多', 'rgba(16,185,129,0.16)', '#065f46'],
    ['🌍 宏观派', 'C', 7, 44, '看空', 'rgba(220,38,38,0.14)', '#991b1b'],
    ['📈 技术派', 'D', 4, 39, '看空', 'rgba(220,38,38,0.14)', '#991b1b'],
    ['🇨🇳 中国价投', 'E', 7, 79, '看多', 'rgba(16,185,129,0.16)', '#065f46'],
    ['⚡ A 股游资', 'F', 24, 47, '看空', 'rgba(220,38,38,0.14)', '#991b1b'],
    ['🤖 量化', 'G', 4, 55, '中性', 'rgba(148,163,184,0.20)', '#475569'],
    ['🔬 科技领袖派', 'H', 4, 72, '看多', 'rgba(16,185,129,0.16)', '#065f46'],
    ['🎯 AI 卡位猎手', 'I', 1, 66, '看多', 'rgba(16,185,129,0.16)', '#065f46']
  ];
  afterComment('INJECT_SCHOOL_SCORES',
    '<div style="font-size:11px;color:#7c3aed;letter-spacing:2px;margin-bottom:4px">'
    + '🎭 SCHOOL SCORES · 九大流派各自评分</div>'
    + '<div style="font-size:12px;color:#64748b;margin-bottom:14px">'
    + '混合打分 = 0.65 × 实分均值 + 0.35 × 投票共识 · 再做极化拉伸(k=1.3) · '
    + '不同哲学给出不同分数 · 分歧越大意味着结论越不稳 · 鼠标悬停查看分量</div>'
    + '<div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:12px">'
    + SCHOOLS.map(function (s) {
        var mean = (s[3] * 0.94).toFixed(1);
        var vote = (s[3] * 1.08).toFixed(1);
        return '<div style="background:' + s[5] + ';border-radius:8px;padding:14px 16px;'
          + 'border:1px solid rgba(16,24,40,0.05)" title="score_mean=' + mean
          + ' · vote_weighted=' + vote + ' · 极化后 ' + s[3] + '">'
          + '<div style="display:flex;justify-content:space-between;align-items:baseline">'
          + '<div style="font-weight:600;font-size:14px;color:' + s[6] + '">' + s[0]
          + ' <span style="font-weight:400;font-size:11px;color:#94a3b8">· ' + s[2] + ' 人</span></div>'
          + '<div style="font-size:11px;color:' + s[6] + ';font-weight:600;letter-spacing:1px">' + s[4] + '</div>'
          + '</div><div style="margin-top:6px;font-size:11px;color:#64748b">混合分 '
          + '<strong style="font-size:16px;color:' + s[6] + '">' + s[3] + '</strong>/100</div></div>';
      }).join('') + '</div>');

  /* 3.7 · 22 维扫描（6 个分组） */
  var DIMS = [
    [1, '财报扎实度', 'Financials', 8, 5, '毛利率 91.5% · 净利率 52.1% · 现金流/净利 1.12x'],
    [2, '盈利质量', 'Profitability', 9, 4, 'ROE 32.4% · 连续 12 年 > 25%'],
    [3, '成长性', 'Growth', 5, 4, '营收 +9.8% · 净利 +11.2% · 增速中枢下移'],
    [4, '现金流', 'Cash Flow', 9, 4, '经营性现金流 682 亿 · 自由现金流 594 亿'],
    [5, '趋势结构', 'Trend', 6, 4, 'MA20 上穿 MA60 · 距 5 年高点 -18%'],
    [6, '动量', 'Momentum', 6, 3, '20 日 +6.4% · 相对沪深300 +4.1%'],
    [7, '量能', 'Volume', 6, 3, '5 日均量 +12% · 换手 0.41%'],
    [8, '波动率', 'Volatility', 7, 2, '年化波动 26.8% · 低于行业均值'],
    [9, '估值锚', 'Valuation', 4, 5, 'PE 22.4x · 5 年分位 31% · 股息率 3.2%'],
    [10, '同行对比', 'Peers', 9, 4, '市值排名 #1 · 毛利率领先行业 34pct'],
    [11, '上下游产业链', 'Supply Chain', 8, 3, '上游粮食议价力强 · 下游渠道掌控力极强'],
    [12, '竞争格局', 'Competition', 9, 4, '高端白酒 CR3 约 62% · 格局稳定'],
    [13, '护城河 (5 类)', 'Moat', 9, 3, '品牌 10/10 · 渠道 9/10 · 转换成本 8/10'],
    [14, '管理层与治理', 'Governance', 8, 3, '分红率 51.9% · 无质押 · 关联交易透明'],
    [15, '宏观环境', 'Macro', 5, 3, '社零 +3.2% · 白酒产量 -6.1% · 库存去化中'],
    [16, '政策与监管', 'Policy', 7, 2, '消费税预期稳定 · 无新增限制'],
    [17, '行业景气', 'Industry', 5, 4, '行业动销 -4% · 批价同比 -8%'],
    [18, '舆情与大V', 'Sentiment', 7, 3, '看多占比 68% · 热搜第 4 位'],
    [19, '事件驱动', 'Events', 6, 3, '三季报预告 · 中秋动销超预期'],
    [20, '杀猪盘检测', 'Trap Scan', 10, 5, '🟢 安全 · 风险分 0/100'],
    [21, '黑天鹅风险', 'Tail Risk', 6, 2, '未发现重大尾部风险敞口'],
    [22, '机构建模', 'Modeling', 6, 3, 'DCF 内在价值 ¥1,742 · 安全边际 +9.8%']
  ];
  function dimCard(d) {
    var lvl = d[3] >= 8 ? 'high' : d[3] >= 5 ? 'mid' : 'low';
    var stars = '★'.repeat(d[4]) + '☆'.repeat(5 - d[4]);
    return '<div class="dim-card" data-dim="' + String(d[0]).padStart(2, '0') + '">'
      + '<div class="dim-head"><div>'
      + '<div class="dim-num">DIM ' + String(d[0]).padStart(2, '0') + ' · WEIGHT ' + stars + '</div>'
      + '<div class="dim-title">' + d[1] + '</div>'
      + '<div class="dim-en">' + d[2] + '</div></div>'
      + '<div class="dim-score"><div class="num ' + lvl + '">' + d[3] + '</div></div></div>'
      + '<div class="dim-bar"><div class="fill ' + lvl + '" style="width: ' + (d[3] * 10) + '%"></div></div>'
      + '<div class="dim-label">' + esc(d[5]) + '</div>'
      + '<div class="dim-source">数据来源: <span class="badge live">官方接口</span></div></div>';
  }
  var ROWS = [[0, 5], [5, 9], [9, 13], [13, 17], [17, 19], [19, 22]];
  ROWS.forEach(function (range, i) {
    slot('.dim-row', DIMS.slice(range[0], range[1]).map(dimCard).join(''), i);
  });

  /* 3.8 · 机构建模 */
  slot('.inst-modeling-wrap',
    '<div class="dcf-block"><div style="background:#fff;border:1px solid #e7ecf2;border-radius:12px;'
    + 'padding:18px 20px;margin:14px 0"><div style="font-weight:700;font-size:14px;color:#0f172a;'
    + 'margin-bottom:10px">DCF 现金流折现 · 两阶段模型</div>'
    + '<div style="display:flex;gap:28px;flex-wrap:wrap;font-size:12px;color:#475569">'
    + '<div>WACC <strong>9.2%</strong></div><div>永续增长 <strong>3.0%</strong></div>'
    + '<div>显性期 <strong>5 年</strong></div>'
    + '<div>内在价值 <strong style="color:#065f46;font-size:15px">¥1,742</strong></div>'
    + '<div>当前价 <strong>¥1,586</strong></div>'
    + '<div>安全边际 <strong style="color:#065f46">+9.8%</strong></div></div></div></div>'
    + '<div class="comps-block"><div style="background:#fff;border:1px solid #e7ecf2;border-radius:12px;'
    + 'padding:18px 20px;margin:14px 0"><div style="font-weight:700;font-size:14px;color:#0f172a;'
    + 'margin-bottom:10px">Comps 可比公司</div>'
    + '<table style="width:100%;border-collapse:collapse;font-size:12px;color:#475569">'
    + '<tr style="color:#64748b"><th style="text-align:left;padding:6px 0">公司</th>'
    + '<th style="text-align:right">PE</th><th style="text-align:right">PB</th>'
    + '<th style="text-align:right">ROE</th><th style="text-align:right">股息率</th></tr>'
    + ['贵州茅台|22.4|7.8|32.4%|3.2%', '五粮液|16.8|4.1|24.6%|4.1%',
       '泸州老窖|18.2|5.3|28.1%|3.6%', '山西汾酒|21.5|6.2|27.3%|2.4%']
        .map(function (r) {
          var c = r.split('|');
          return '<tr style="border-top:1px solid #eef2f7"><td style="padding:6px 0">' + c[0]
            + '</td><td style="text-align:right">' + c[1] + '</td>'
            + '<td style="text-align:right">' + c[2] + '</td>'
            + '<td style="text-align:right">' + c[3] + '</td>'
            + '<td style="text-align:right">' + c[4] + '</td></tr>';
        }).join('')
    + '</table></div></div>');

  /* 3.9 · 风险清单 */
  slot('.risk-box',
    '<ul><li>批价同比 -8% · 渠道库存 2.1 个月，去化速度低于预期</li>'
    + '<li>白酒行业产量连续 6 个季度负增长，需求端尚未见底</li>'
    + '<li>估值 22.4x 处于 5 年 31% 分位，但增速中枢下移会压低合理估值</li>'
    + '<li>消费税改革若落地，静态测算影响净利约 -4% ~ -7%</li>'
    + '<li>高端消费复苏不及预期 · 中秋后进入传统淡季</li></ul>');

  /* 3.10 · 分享卡：投票分布 + Top3
     人数由百分比折算（×66），所以必须与 31/13/22 的分档统计对得上。 */
  var VOTES = [['强烈买入', 3, 'var(--bull-green)'], ['买入', 44, 'var(--bull-green)'],
               ['观望', 20, 'var(--text-dim)'], ['回避', 33, 'var(--bear-red)']];
  slot('.sc-votes', VOTES.map(function (v) {
    return '<div class="sc-vote-row"><span style="width: 140px">' + v[0] + '</span>'
      + '<div class="bar"><div class="fill" style="width:' + v[1] + '%; background:' + v[2] + '"></div></div>'
      + '<span style="width: 60px; text-align: right">' + Math.round(v[1] * 66 / 100) + ' 人</span></div>';
  }).join(''));
  function bestCell(id) {
    return '<div class="sc-best-cell"><img src="avatars/' + id + '.svg"/>'
      + '<div class="name">' + esc(NAMES[id] || id) + '</div>'
      + '<div class="score-num">' + SCORES[id] + '</div></div>';
  }
  // 最高分三人 / 最低分三人 —— 直接从 SCORE_STR 取，不手写
  var ranked = ROSTER.map(function (r) { return r.split('|')[0]; })
    .sort(function (a, b) { return SCORES[b] - SCORES[a]; });
  slot('.sc-best', ranked.slice(0, 3).map(bestCell).join(''), 0);
  slot('.sc-best', ranked.slice(-3).reverse().map(bestCell).join(''), 1);

  /* 3.11 · 小白友好层 */
  slot('.friendly-trio',
    '<div class="friendly-card scenario"><div class="fc-icon">💰</div>'
    + '<div class="fc-title">如果现在买 1 万块</div><div class="fc-body">'
    + '<div style="font-size:11px;color:#475569;margin-bottom:8px">按入场价 <strong>¥1,586.00</strong> 计算：</div>'
    + '<div class="scenario-row"><span class="label">最坏情况 · 5%</span><span class="val down">-32.0% → ¥6,800</span></div>'
    + '<div class="scenario-row"><span class="label">偏差情况 · 25%</span><span class="val down">-13.0% → ¥8,700</span></div>'
    + '<div class="scenario-row"><span class="label">合理情况 · 40%</span><span class="val up">+17.3% → ¥11,730</span></div>'
    + '<div class="scenario-row"><span class="label">乐观情况 · 25%</span><span class="val up">+38.7% → ¥13,870</span></div>'
    + '<div class="scenario-row"><span class="label">极致乐观 · 5%</span><span class="val up">+72.5% → ¥17,250</span></div>'
    + '</div></div>'
    + '<div class="friendly-card similar"><div class="fc-icon">🔗</div>'
    + '<div class="fc-title">跟它最像的另外几只票</div><div class="fc-body">'
    + '<div style="font-size:12px;color:#475569;line-height:1.9">'
    + '五粮液 · 相似度 88%<br>泸州老窖 · 相似度 85%<br>山西汾酒 · 相似度 79%</div></div></div>'
    + '<div class="friendly-card exit"><div class="fc-icon">🚪</div>'
    + '<div class="fc-title">出现这些信号就离场</div><div class="fc-body">'
    + '<div class="exit-trigger-item">股价跌破 ¥1,380（估值底）→ 无条件止损</div>'
    + '<div class="exit-trigger-item">批价连续 3 个月下行 → 定价权受损信号</div>'
    + '<div class="exit-trigger-item">下季度营收同比转负 → 基本面反转信号</div>'
    + '<div class="exit-trigger-item">预收款同比下滑超 15% → 渠道蓄水池见底</div>'
    + '<div class="exit-trigger-item">PE 站上 5 年 90 分位 → 泡沫区获利了结</div>'
    + '</div></div>');

  /* 3.12 · 公募基金持仓 */
  slot('.fund-mgr-section',
    '<div style="padding:24px;text-align:center;color:#94a3b8;font-size:12px">'
    + '（示例数据 · 模板预览模式下不加载真实基金持仓）</div>');
})();
