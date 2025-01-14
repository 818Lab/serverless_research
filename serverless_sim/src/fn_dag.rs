use std::{ cell::{ Ref, RefMut }, collections::{ HashMap, HashSet, VecDeque }, io };

use daggy::{ petgraph::visit::{ Topo, Visitable }, Dag, NodeIndex, Walker };
use enum_as_inner::EnumAsInner;

use crate::{
    config::APPConfig, dag_parsers::csv_parser::parse_dag_csv, mechanism::SimEnvObserve, node::{ EnvNodeExt, NodeId }, request::{ ReqId, Request }, sim_env::SimEnv, CONTAINER_BASIC_MEM
};

pub type FnId = usize;

pub type DagId = usize;

pub type FnDagInner = Dag<FnId, f32>;

#[derive(Clone)]
pub struct FnDAG {
    pub dag_i: DagId,
    pub begin_fn_g_i: NodeIndex,
    pub dag_inner: FnDagInner,
}

impl FnDAG {
    // 初始化一个FnDAG，包含一个起始函数
    fn new(begin_fn: FnId, dag_i: DagId, env: &SimEnv) -> Self {
        // 创建一个空的DAG
        let mut dag = Dag::new();
        // 加入起始函数
        let begin = dag.add_node(begin_fn);

        // 设置这个函数实例的DAGid以及在这个DAG中的位置
        env.func_mut(begin_fn).setup_after_insert_into_dag(dag_i, begin);

        Self {
            dag_i,
            begin_fn_g_i: begin,
            dag_inner: dag,
        }
    }

    // 单函数DAG
    pub fn instance_single_fn(dag_i: DagId, env: &SimEnv) -> FnDAG {
        let begin_fn: FnId = env.fn_gen_rand_fn();
        let dag = FnDAG::new(begin_fn, dag_i, env);
        dag
    }

    // 创建一个复杂DAG实例
    pub fn instance_map_reduce(dag_i: DagId, env: &SimEnv, map_cnt: usize) -> FnDAG {
        // 创建一个Func实例并获得fnid，设置为该DAG的起始函数
        let begin_fn = env.fn_gen_rand_fn();

        // 创建一个FnDAG实例，并设置起始函数
        let mut dag = FnDAG::new(begin_fn, dag_i, env);

        // 创建一个Func实例并获得fnid，设置为该DAG的结束函数
        let end_fn = env.fn_gen_rand_fn();
        // 为DAG添加一个结束函数，并返回它的节点索引
        let end_g_i = dag.dag_inner.add_node(end_fn);

        // 设置这个函数实例的DAGid以及在这个DAG中的位置
        env.func_mut(end_fn).setup_after_insert_into_dag(dag_i, end_g_i);

        // 往DAG图里插入 map_cnt 数量的节点
        for _i in 0..map_cnt {
            // 创建Func实例并获得fnid
            let next = env.fn_gen_rand_fn();

            // 为DAG图插入一个节点，是初始节点的子节点，并返回它的节点索引
            let (_, next_i) = dag.dag_inner.add_child(
                dag.begin_fn_g_i,
                env.core.fns()[begin_fn].out_put_size,
                next
            );
            // 设置这个函数实例的DAGid以及在这个DAG中的位置
            env.func_mut(next).setup_after_insert_into_dag(dag_i, next_i);

            // 为DAG添加边,让中间节点连接到结束节点
            dag.dag_inner.add_edge(next_i, end_g_i, env.func(next).out_put_size).unwrap();
        }

        dag
    }
    // #[allow(dead_code)]
    // pub fn begin_fn(&self) -> FnId {
    //     self.dag[self.begin_fn_g_i]
    // }

    // 从 CSV 文件生成 DAG 的函数
    pub fn dag_from_csv(dag_i: DagId, env: &SimEnv) -> FnDAG {
        // 解析 CSV 文件获取任务数据
        let tasks = parse_dag_csv(env).expect("Failed to parse CSV file");
    
        // 初始化一个空的 FnDAG
        let mut dag = FnDAG {
            dag_i,
            begin_fn_g_i: NodeIndex::new(0), 
            dag_inner: FnDagInner::new(),
        };
    
        // 存储任务名到节点索引的映射
        let mut task_nodes = HashMap::new();
    
        // 添加任务节点到 DAG 中
        for task in &tasks {
            let fn_id = env.fn_gen_rand_fn(); // 为任务生成唯一的 FnId
            let task_node = dag.dag_inner.add_node(fn_id); // 将任务添加为节点
            task_nodes.insert(task.task_id, task_node); // 存储节点索引
            // 设置任务的 DAGId 和位置
            env.func_mut(fn_id).setup_after_insert_into_dag(dag_i, task_node);

            // if task.task_id == 1 {
            //     dag.begin_fn_g_i = task_node;
            // }
        }

        for task in &tasks {
            if task.dependencies.is_empty() {
                dag.begin_fn_g_i = *task_nodes
                    .get(&task.task_id)
                    .expect("Task node not found for dependency-free task");
                break; // 找到一个即可退出
            }
        }
    
        // 建立节点之间的依赖关系
        for task in &tasks {
            let task_node = task_nodes.get(&task.task_id).expect("Task node not found");
    
            for dependency_id in &task.dependencies {
                if let Some(dep_node) = task_nodes.get(dependency_id) {
                    let dep_fn_id = dag.dag_inner[*dep_node];
                    // 在 DAG 中添加从依赖节点到当前任务节点的边
                    dag.dag_inner
                        .add_edge(*dep_node, *task_node, env.func(dep_fn_id).out_put_size) // 权重可以动态化
                        .expect("Failed to add edge");
                }
            }
        }
        // 返回构建完成的 DAG
        dag
    }
    
    

    pub fn new_dag_walker(&self) -> Topo<NodeIndex, <FnDagInner as Visitable>::Map> {
        Topo::new(&self.dag_inner)
    }

    pub fn contains_fn(&self, env: &SimEnv, fnid: FnId) -> bool {
        let gi = env.func(fnid).graph_i;
        self.dag_inner.node_weight(gi).is_some() && self.dag_inner[gi] == fnid
    }
}

#[derive(Clone)]
pub struct Func {
    pub fn_id: FnId,

    pub dag_id: DagId,

    pub graph_i: NodeIndex,

    // #  #运算量/s 一个普通请求处理函数请求的运算量为1，
    pub cpu: f32, // 1
    // # 平均时间占用内存资源 mb
    pub mem: f32, // = 300
    // // # 依赖的数据库-整个过程中数据传输量
    // databases_2_throughput={}
    // # 输出数据量 mb
    pub out_put_size: f32, //=100,

    // 当前函数有实例的节点
    pub nodes: HashSet<usize>,

    // frame count of cold start
    pub cold_start_time: usize,

    pub cold_start_container_mem_use: f32,

    pub cold_start_container_cpu_use: f32,
}

impl Func {
    pub fn sub_fns(&self, env: &impl EnvFnExt) -> Vec<FnId> {
        let dag = env.dag_inner(self.dag_id);
        let ps = dag.children(self.graph_i);
        ps.iter(&dag)
            .map(|(_edge, graph_i)| dag[graph_i])
            .collect()
    }

    pub fn parent_fns(&self, env: &impl EnvFnExt) -> Vec<FnId> {
        let dag = env.dag_inner(self.dag_id);
        let ps = dag.parents(self.graph_i);
        ps.iter(&dag)
            .map(|(_edge, graph_i)| dag[graph_i])
            .collect()
    }

    // 设置这个函数实例的DAGid以及在这个DAG中的位置
    pub fn setup_after_insert_into_dag(&mut self, dag_i: DagId, graph_i: NodeIndex) {
        self.dag_id = dag_i;
        self.graph_i = graph_i;
    }

    // 容器启动时,分配的内存
    pub fn container_mem(&self) -> f32 {
        CONTAINER_BASIC_MEM
    }
}

#[derive(EnumAsInner, Clone)]
pub enum FnContainerState {
    // 创建
    Starting {
        left_frame: usize,
    },
    // 运行
    Running,
}

#[derive(Clone)]
pub struct FnContainer {
    pub node_id: NodeId,
    pub fn_id: FnId,
    pub req_fn_state: HashMap<ReqId, RunningTask>,
    pub born_frame: usize,
    pub used_times: usize,
    pub this_frame_used: bool,
    pub recent_frames_done_cnt: VecDeque<usize>,
    pub recent_frames_working_cnt: VecDeque<usize>,

    /// cpu 利用率
    /// 实际用的计算量/分配到的cpu计算量
    cpu_use_rate: f32,
    pub last_frame_cpu_used: f32,

    // mem使用量
    pub mem_use: f32,
    pub last_frame_mem: f32,

    state: FnContainerState,
}

const DONE_CNT_WINDOW: usize = 20;
const WORKING_CNT_WINDOW: usize = 20;

impl FnContainer {
    pub fn mem_take(&self, env: &SimEnv) -> f32 {
        match self.state() {
            FnContainerState::Starting { .. } => env.func(self.fn_id).cold_start_container_mem_use,
            FnContainerState::Running => env.func(self.fn_id).container_mem(),
        }
    }

    pub fn recent_handle_speed(&self) -> f32 {
        if self.recent_frames_done_cnt.len() == 0 {
            return 0.0;
        }
        (
            self.recent_frames_done_cnt
                .iter()
                .map(|v| *v)
                .sum::<usize>() as f32
        ) / (self.recent_frames_done_cnt.len() as f32)
    }
    pub fn busyness(&self) -> f32 {
        if self.recent_frames_working_cnt.len() == 0 {
            return 0.0;
        }
        let mut weight = 1;
        self.recent_frames_working_cnt
            .iter()
            .map(|v| {
                let v = (*v * weight) as f32;
                // 越接近当前权重越大
                weight += 1;
                v
            })
            .sum::<f32>() / (self.recent_frames_working_cnt.len() as f32)
    }

    // 判断一定帧数内该容器是否空闲
    pub fn recent_frame_is_idle(&self, mut frame_cnt: usize) -> bool {
        for working_cnt in self.recent_frames_working_cnt.iter().rev() {
            if *working_cnt > 0 {
                return false;
            }
            frame_cnt -= 1;
            if frame_cnt == 0 {
                break;
            }
        }
        true
    }

    pub fn record_this_frame(&mut self, _sim_env: &SimEnv, done_cnt: usize, working_cnt: usize) {
        // log::info!(
        //     "container record at frame: {} done cnt:{done_cnt} working cnt:{working_cnt}",
        //     sim_env.current_frame()
        // );
        self.recent_frames_done_cnt.push_back(done_cnt);
        while self.recent_frames_done_cnt.len() > DONE_CNT_WINDOW {
            self.recent_frames_done_cnt.pop_front();
        }
        self.recent_frames_working_cnt.push_back(working_cnt);
        while self.recent_frames_working_cnt.len() > WORKING_CNT_WINDOW {
            self.recent_frames_working_cnt.pop_front();
        }
    }

    pub fn new(fn_id: FnId, node_id: NodeId, sim_env: &SimEnv) -> Self {
        Self {
            node_id,
            fn_id,
            req_fn_state: HashMap::default(),
            born_frame: sim_env.current_frame(),
            used_times: 0,
            this_frame_used: false,
            cpu_use_rate: 0.0,
            last_frame_cpu_used: 0.0,
            mem_use: CONTAINER_BASIC_MEM,
            last_frame_mem: 0.0,
            state: FnContainerState::Starting {
                left_frame: sim_env.func(fn_id).cold_start_time,
            },
            recent_frames_done_cnt: VecDeque::new(),
            recent_frames_working_cnt: VecDeque::new(),
        }
    }

    pub fn starting_left_frame_move_on(&mut self, env: &SimEnv) {
        let mut to_running = false;
        match self.state {
            FnContainerState::Starting { ref mut left_frame } => {
                *left_frame -= 1;
                if *left_frame == 0 {
                    // drop(left_frame);

                    to_running = true;
                }
            }
            _ => {
                panic!("not starting");
            }
        }
        if to_running {
            self.state = FnContainerState::Running;
            env.on_fnins_cold_started(self);
        }
    }

    pub fn container_basic_mem(&self, env: &SimEnv) -> f32 {
        match self.state {
            FnContainerState::Starting { .. } => env.func(self.fn_id).cold_start_container_mem_use,
            FnContainerState::Running => CONTAINER_BASIC_MEM,
        }
    }

    // pub fn calc_mem_used(&self, env: &SimEnv) -> f32 {
    //     match self.state {
    //         FnContainerState::Starting { .. } => env.func(self.fn_id).cold_start_container_mem_use,
    //         FnContainerState::Running => {
    //             CONTAINER_BASIC_MEM + env.func(self.fn_id).mem * self.req_fn_state.len() as f32
    //         }
    //     }
    // }

    pub fn use_freq(&self, env: &SimEnv) -> f32 {
        if env.current_frame() - self.born_frame == 0 {
            return 0.0;
        }
        (self.used_times as f32) / ((env.current_frame() - self.born_frame) as f32)
    }

    pub fn cpu_use_rate(&self) -> f32 {
        self.cpu_use_rate
    }

    pub fn set_cpu_use_rate(&mut self, alloced: f32, used: f32) {
        if alloced < 0.00001 {
            panic!("alloced cpu is too small");
            // self.cpu_use_rate = 0.0;
        }
        self.cpu_use_rate = used / alloced;
    }

    pub fn state_mut(&mut self) -> &mut FnContainerState {
        &mut self.state
    }

    pub fn state(&self) -> &FnContainerState {
        &self.state
    }

    pub fn is_running(&self) -> bool {
        match self.state {
            FnContainerState::Running => true,
            _ => false,
        }
    }

    pub fn is_idle(&self) -> bool {
        match self.state {
            FnContainerState::Running => self.req_fn_state.len() == 0,
            FnContainerState::Starting { .. } => false,
        }
    }
}

#[derive(Clone)]
pub struct RunningTask {
    /// nodeid - (need,recv)
    pub data_recv: HashMap<NodeId, (f32, f32)>,

    /// 剩余计算量
    pub left_calc: f32,
}

impl RunningTask {
    pub fn data_recv_done(&self) -> bool {
        let mut done = true;
        for (_, (need, recv)) in self.data_recv.iter() {
            if *need - *recv > 0.00001 {
                done = false;
                break;
            }
        }
        done
    }

    pub fn compute_done(&self) -> bool {
        self.left_calc <= 0.0
    }
}

impl SimEnv {
    // 创建一个Func实例
    fn fn_gen_rand_fn(&self) -> FnId {
        // 获得fnid
        let id = self.help.fn_next_id();
        // 根据不同的函数类型确定cpu和输出数据量
        let (cpu, out_put_size) = if self.help.config().fntype_cpu() {
            (self.env_rand_f(10.0, 100.0), self.env_rand_f(0.1, 20.0))
        } else if self.help.config().fntype_data() {
            (self.env_rand_f(10.0, 100.0), self.env_rand_f(30.0, 100.0))
        } else {
            panic!("not support fntype");
        };

        // 创建一个Func实例并加入到core中
        self.core.fns_mut().push(Func {
            fn_id: id,
            cpu,
            mem: self.env_rand_f(100.0, 1000.0),
            out_put_size,
            nodes: HashSet::new(),
            cold_start_container_mem_use: 100.0,
            cold_start_container_cpu_use: self.env_rand_f(0.1, 1.0),
            cold_start_time: self.env_rand_i(50, 100),
            dag_id: 0,
            graph_i: (0).into(),
        });
        id
    }

    fn gen_dags_for_apptype(&self, apptype: &APPConfig, env: &SimEnv) {
        for _ in 0..apptype.app_cnt {
            match &*apptype.dag_type {
                "single" => {
                    let dag_i = self.core.dags().len();
                    // (appconfig: &APPConfig, dag_i: DagId, env: &SimEnv)
                    let dag = FnDAG::instance_single_fn(dag_i, env);
                    self.core.dags_mut().push(dag);

                    // let mapcnt = env.env_rand_i(2, 5); //2-4
                    //         let dag_i = env.dags.borrow().len();
                    //         let dag = FnDAG::instance_map_reduce(dag_i, env, mapcnt);
                }
                "chain" => {}
                "branch" => {
                    let mapcnt = self.env_rand_i(2, 5); //2-4
                    let dag_i = self.core.dags().len();
                    let dag = FnDAG::instance_map_reduce(dag_i, env, mapcnt);
                    self.core.dags_mut().push(dag);
                }
                _ => {
                    panic!("not support");
                }
            }
        }
    }

    pub fn fn_gen_fn_dags(&self, _env: &SimEnv) {
        let env = self;

        // for _ in 0..10 {
        //     let dag_i = env.dags.borrow().len();
        //     let dag = FnDAG::instance_map_reduce(dag_i, env, util::rand_i(2, 10));
        //     env.dags.borrow_mut().push(dag);
        // }
        // for app in &self.config.app_types {
        //     // (&self, apptype: &APPConfig, env: &SimEnv)
        //     self.gen_dags_for_apptype(app, env);
        // }

        // 检查配置中的dag_type
        if self.help.config().dag_type_dag() {
            // 如果dag_type为dag，则创建 33 个具有多个子节点的复杂DAG实例
            for _ in 0..33 {
                // 随机确定每个图中节点的数量
                let mapcnt = env.env_rand_i(2, 5); //2-4
                let dag_i = env.core.dags().len();

                // 创建一个复杂DAG实例
                // let dag = FnDAG::instance_map_reduce(dag_i, env, mapcnt);
                let dag = FnDAG::dag_from_csv(dag_i, env);
                // log::info!("dag {} {:?}", dag.dag_i, dag.dag_inner);

                env.core.dags_mut().push(dag);
            }
        } else if
            // 如果dag_type为single，则创建10个只包含单个节点的简单DAG实例
            self.help.config().dag_type_single()
        {
            for _ in 0..100 {
                let dag_i = env.core.dags().len();

                // 创建一个简单DAG实例
                let dag = FnDAG::instance_single_fn(dag_i, env);
                env.core.dags_mut().push(dag);
            }
        } else if
            // 跑指标2的实验用
            self.help.config().dag_type_mix()
        {
            for i in 0..50 {
                if i >= 20 {
                    // 随机确定每个图中节点的数量
                    let mapcnt = env.env_rand_i(2, 5); //2-4
                    let dag_i = env.core.dags().len();

                    // 创建一个复杂DAG实例
                    // let dag = FnDAG::instance_map_reduce(dag_i, env, mapcnt);
                    let dag = FnDAG::dag_from_csv(dag_i, env);
                    // log::info!("dag {} {:?}", dag.dag_i, dag.dag_inner);

                    env.core.dags_mut().push(dag);
                } else {
                    let dag_i = env.core.dags().len();

                    // 创建一个简单DAG实例
                    let dag = FnDAG::instance_single_fn(dag_i, env);
                    env.core.dags_mut().push(dag);
                }
            }
        } else {
            panic!("not support dag type {}", self.help.config().dag_type);
        }
    }

    // pub fn fn_is_fn_dag_begin(&self, dag_i: DagId, fn_i: FnId) -> bool {
    //     let dags = self.dags.borrow();
    //     let dag = &dags[dag_i];
    //     dag.dag[dag.begin_fn_g_i] == fn_i
    // }

    pub fn fn_new_fn_running_state(&self, req: &Request, fnid: FnId) -> RunningTask {
        // log::info!("reqid {}, fnid {}. frame {} new running task", req.req_id, fnid, self.current_frame());
        let env = self;

        let total_calc: f32 = env.func(fnid).cpu;
        let fngi = env.func(fnid).graph_i;
        let mut need_node_data: HashMap<NodeId, f32> = HashMap::new();
        let dag_i = req.dag_i;
        let env_dags = env.core.dags();
        let dag = &env_dags[dag_i];
        for (_, pgi) in dag.dag_inner.parents(fngi).iter(&dag.dag_inner) {
            let p: FnId = dag.dag_inner[pgi];
            let node = req.get_fn_node(p).unwrap();
            need_node_data
                .entry(node)
                .and_modify(|v| {
                    *v += env.core.fns()[p].out_put_size;
                })
                .or_insert(env.core.fns()[p].out_put_size);
        }
        RunningTask {
            data_recv: need_node_data
                .iter()
                .map(|(node_id, data)| (*node_id, (*data, 0.0)))
                .collect(),

            left_calc: total_calc,
        }
    }

    // pub fn fn_running_containers_nodes(&self, fnid: FnId) -> HashSet<NodeId> {
    //     let mut nodes = HashSet::<NodeId>::new();
    //     self.fn_containers_for_each(fnid, |c| {
    //         if c.state().is_running() {
    //             nodes.insert(c.node_id);
    //         }
    //     });
    //     nodes
    // }
}

impl EnvFnExt for SimEnv {}
impl EnvFnExt for SimEnvObserve {}

pub trait EnvFnExt: EnvNodeExt {
    fn func<'a>(&'a self, i: FnId) -> Ref<'a, Func> {
        let b = self.core().fns();

        Ref::map(b, |vec| &vec[i])
    }

    fn func_mut<'a>(&'a self, i: FnId) -> RefMut<'a, Func> {
        let fns = self.core().fns_mut();

        RefMut::map(fns, |fns| &mut fns[i])
    }

    fn dag_inner<'a>(&'a self, i: usize) -> Ref<'a, FnDagInner> {
        let b = self.core().dags();

        Ref::map(b, |vec| &vec[i].dag_inner)
    }

    fn dag<'a>(&'a self, i: usize) -> Ref<'a, FnDAG> {
        let b = self.core().dags();

        Ref::map(b, |vec| &vec[i])
    }

    fn fn_container_cnt(&self, fnid: FnId) -> usize {
        let map = self.core().fn_2_nodes();
        map.get(&fnid).map_or_else(
            || 0,
            |nodes| nodes.len()
        )
    }

    fn fn_containers_for_each<F: FnMut(&FnContainer)>(&self, fnid: FnId, mut f: F) {
        let map = self.core().fn_2_nodes();
        if let Some(nodes) = map.get(&fnid) {
            for node in nodes.iter() {
                let node = self.node(*node);
                f(&node.container(fnid).unwrap());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_env::SimEnv; // 假设 SimEnv 在 `sim_env` 模块下
    use crate::fn_dag::{FnDAG, DagId}; // 假设 FnDAG 在 `dag` 模块下
    use crate::config::Config; // 假设有 Config 配置文件
    
    /// 打印节点的基本信息（FnId, 父节点和子节点）
    fn print_node_info(dag: &FnDAG, node_idx: petgraph::graph::NodeIndex) {
        let fn_id = dag.dag_inner[node_idx];
        println!("Node {:?} - FnId: {}", node_idx, fn_id);

        // 打印子节点
        let children: Vec<_> = dag
            .dag_inner
            .graph()
            .neighbors_directed(node_idx, petgraph::Direction::Outgoing)
            .collect();
        if !children.is_empty() {
            println!("  Children: {:?}", children);
        }

        // 打印父节点
        let parents: Vec<_> = dag
            .dag_inner
            .graph()
            .neighbors_directed(node_idx, petgraph::Direction::Incoming)
            .collect();
        if !parents.is_empty() {
            println!("  Parents: {:?}", parents);
        }
    }

    #[test]
    fn test_instance_map_reduce() {
        let env = SimEnv::new(Config::new_test()); // 初始化 SimEnv
        let dag_id = 1; // DAG ID
        let map_cnt = 3; // 设置 map 节点的数量

        // 调用 `instance_map_reduce` 生成 DAG
        let dag = FnDAG::dag_from_csv(dag_id, &env);

        println!("DAG Information:");

        // 打印 DAG 的基本信息
        println!("Total Nodes: {}", dag.dag_inner.node_count());
        println!("Total Edges: {}", dag.dag_inner.edge_count());
        println!("Begin Node Index: {:?}", dag.begin_fn_g_i);

        // 打印每个节点的信息
        println!("Nodes and Edges:");
        for node_idx in dag.dag_inner.graph().node_indices() {
            print_node_info(&dag, node_idx);
        }

        // 可选：添加断言，验证 DAG 是否符合预期
        assert!(dag.dag_inner.node_count() > 0, "DAG should have at least one node");
        assert!(dag.dag_inner.edge_count() > 0, "DAG should have at least one edge");

        // 可选：确保开始节点的索引正确
        // assert_eq!(dag.begin_fn_g_i.index(), 0, "Begin node index should be 0");

        // 可选：检查生成的 map 节点数是否匹配
        // assert_eq!(
        //     dag.dag_inner.node_count() - 2,  // -2 for begin and end nodes
        //     map_cnt,
        //     "The number of map nodes should match the input map_cnt"
        // );
    }
}

