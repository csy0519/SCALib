//! Python binding of SCALib's FactorGraph rust implementation.
//语句导入了Rust中使用的模块和功能。
use std::collections::HashMap;
use std::sync::Arc;

use bincode::{deserialize, serialize};
use numpy::{PyArray, PyArray1, PyArray2};
use pyo3::exceptions::{PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyTuple};

use scalib::sasca;
//定义了一个Python模块中的类，这意味着FactorGraph类将在Python中作为_scalib_ext.FactorGraph可用。
#[pyclass(module = "_scalib_ext")]
pub(crate) struct FactorGraph {
    inner: Option<Arc<sasca::FactorGraph>>,
    //inner 是 FactorGraph 结构体的一个私有字段，它是 Option<Arc<sasca::FactorGraph>> 类型的。
}
impl FactorGraph {
    //get_inner 用于获取结构体内部的Arc引用。如果inner是None，它会在运行时失败
    fn get_inner(&self) -> &Arc<sasca::FactorGraph> {
        self.inner.as_ref().unwrap()
    }
    //get_factor 方法试图从因子图中获取一个因子的标识符。如果找到了这个因子，它就返回这个因子的ID；如果没有找到，它就返回一个错误。
    fn get_factor(&self, factor: &str) -> PyResult<sasca::FactorId> {
        self.get_inner()
            .get_factorid(factor)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// TODO run stuff on SCALib thread pool
//指明接下来的方法将被暴露给Python。两个impl FactorGraph区分用于Rust内部逻辑的方法和暴露给Python的方法
#[pymethods]
impl FactorGraph {
    #[new]
    //定义了构造函数，用于在Python中创建FactorGraph对象。
    //如果提供了参数，那么new函数会从这些参数中提取因子图的描述和表格。表格数据将被用来在Rust中构建一个因子图
    #[pyo3(signature = (*args))]
    fn new(args: &PyTuple) -> PyResult<Self> {
        if args.len() == 0 {
            Ok(Self { inner: None })
        } else {
            let (description, tables): (
                &str,
                std::collections::HashMap<String, &PyArray1<sasca::ClassVal>>,
            ) = args.extract()?;
            let tables = tables
                .into_iter()
                .map(|(k, v)| PyResult::<_>::Ok((k, PyArray::to_vec(v)?)))
                .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
            let fg = sasca::build_graph(description, tables)
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            Ok(Self {
                inner: Some(Arc::new(fg)),
            })
        }
    }
//__getstate__方法用于序列化，序列化是指将对象状态转换为可存储或传输的格式的过程
    pub fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        let to_ser: Option<&sasca::FactorGraph> = self.inner.as_deref();
        Ok(PyBytes::new(py, &serialize(&to_ser).unwrap()).to_object(py))
    }

    pub fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
        match state.extract::<&PyBytes>(py) {
            Ok(s) => {
                let deser: Option<sasca::FactorGraph> = deserialize(s.as_bytes()).unwrap();
                self.inner = deser.map(Arc::new);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
    //允许Python代码初始化一个信念传播的过程，返回一个包含新创建的BPState对象的PyResult
    pub fn new_bp(&self, py: Python, nmulti: u32, public_values: PyObject) -> PyResult<BPState> {
        //let关键字用于创建变量并将其初始化为特定的值，?表示如果出现错误则返回错误
        let pub_values = pyobj2pubs(py, public_values, self.get_inner().public_multi())?;
        Ok(BPState {
            inner: Some(sasca::BPState::new(
                self.get_inner().clone(),
                nmulti,
                pub_values,
            )),
        })
    }

    pub fn var_names(&self) -> Vec<&str> {
        self.get_inner().var_names().collect()
    }
    pub fn factor_names(&self) -> Vec<&str> {
        self.get_inner().factor_names().collect()
    }
    pub fn factor_scope<'s>(&'s self, factor: &str) -> PyResult<Vec<&'s str>> {
        let factor_id = self.get_factor(factor)?;
        Ok(self
            .get_inner()
            .factor_scope(factor_id)
            .map(|v| self.get_inner().var_name(v))
            .collect())
    }
    //用于进行信念传播状态的合法性检查
    pub fn sanity_check(
        &self,
        py: Python,
        public_values: PyObject,
        var_assignments: PyObject,
    ) -> PyResult<()> {
        let inner = self.get_inner();
        let pub_values = pyobj2pubs(py, public_values, inner.public_multi())?;
        let var_values = pyobj2pubs(
            py,
            var_assignments,
            inner.vars().map(|(v, vn)| (vn, inner.var_multi(v))),
        )?;
        inner
            .sanity_check(pub_values, var_values.into())
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
}
//将 Python 对象表示的公共值转换为 Rust 中的公共值
fn pyobj2pubs<'a>(
    py: Python,
    public_values: PyObject,
    expected: impl Iterator<Item = (&'a str, bool)>,
) -> PyResult<Vec<sasca::PublicValue>> {
    let mut public_values: HashMap<&str, PyObject> = public_values.extract(py)?;
    let pubs = expected
        .map(|(pub_name, multi)| {
            obj2pub(
                py,
                public_values
                    .remove(pub_name)
                    .ok_or_else(|| PyKeyError::new_err(format!("Missing value {}.", pub_name)))?,
                multi,
            )
        })
        .collect::<Result<Vec<sasca::PublicValue>, PyErr>>()?;
    if public_values.is_empty() {
        Ok(pubs)
    } else {
        let unknown_pubs = public_values.keys().collect::<Vec<_>>();
        Err(PyKeyError::new_err(if unknown_pubs.len() == 1 {
            format!("{} is not a public.", unknown_pubs[0])
        } else {
            format!("{:?} are not publics.", unknown_pubs)
        }))
    }
}

#[pyclass(module = "_scalib_ext")]
pub(crate) struct BPState {
    inner: Option<sasca::BPState>,
}
impl BPState {
    fn get_inner(&self) -> &sasca::BPState {
        self.inner.as_ref().unwrap()
    }
    //作用是允许修改BPState内部的状态
    fn get_inner_mut(&mut self) -> &mut sasca::BPState {
        self.inner.as_mut().unwrap()
    }
    fn get_var(&self, var: &str) -> PyResult<sasca::VarId> {
        self.get_inner()
            .get_graph()
            .get_varid(var)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
    fn get_factor(&self, factor: &str) -> PyResult<sasca::FactorId> {
        self.get_inner()
            .get_graph()
            .get_factorid(factor)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
    fn get_edge(&self, var: sasca::VarId, factor: sasca::FactorId) -> PyResult<sasca::EdgeId> {
        self.get_inner()
            .get_graph()
            .edge(var, factor)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
    fn get_edge_named(&self, var: &str, factor: &str) -> PyResult<sasca::EdgeId> {
        self.get_edge(self.get_var(var)?, self.get_factor(factor)?)
    }
}

#[pymethods]
impl BPState {
    #[new]
    #[pyo3(signature = (*_args))]
    fn new(_args: &PyTuple) -> PyResult<Self> {
        Ok(Self { inner: None })
    }

    pub fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        Ok(PyBytes::new(py, &serialize(&self.inner).unwrap()).to_object(py))
    }

    pub fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
        match state.extract::<&PyBytes>(py) {
            Ok(s) => {
                self.inner = deserialize(s.as_bytes()).unwrap();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn is_cyclic(&self) -> bool {
        self.get_inner().is_cyclic()
    }
    //先验概率
    pub fn set_evidence(&mut self, py: Python, var: &str, distr: PyObject) -> PyResult<()> {
        let var_id = self.get_var(var)?;
        let bp = self.get_inner_mut();//返回inner 字段的可变引用 &mut sasca::BPState
        let distr = obj2distr(py, distr, bp.get_graph().var_multi(var_id))?;
        bp.set_evidence(var_id, distr)
            .map_err(|e| PyTypeError::new_err(e.to_string()))?;
        Ok(())
    }
    pub fn drop_evidence(&mut self, var: &str) -> PyResult<()> {
        let var_id = self.get_var(var)?;
        self.get_inner_mut().drop_evidence(var_id);
        Ok(())
    }
    pub fn get_state(&self, py: Python, var: &str) -> PyResult<PyObject> {
        distr2py(py, self.get_inner().get_state(self.get_var(var)?))
    }
    pub fn set_state(&mut self, py: Python, var: &str, distr: PyObject) -> PyResult<()> {
        let var_id = self.get_var(var)?;
        let bp = self.get_inner_mut();
        let distr = obj2distr(py, distr, bp.get_graph().var_multi(var_id))?;
        bp.set_state(var_id, distr)
            .map_err(|e| PyTypeError::new_err(e.to_string()))?;
        Ok(())
    }
    pub fn drop_state(&mut self, var: &str) -> PyResult<()> {
        let var_id = self.get_var(var)?;
        self.get_inner_mut().drop_state(var_id);
        Ok(())
    }
    //获取从指定因子到给定变量的信念。
    pub fn get_belief_to_var(&self, py: Python, var: &str, factor: &str) -> PyResult<PyObject> {
        let edge_id = self.get_edge_named(var, factor)?;
        distr2py(py, self.get_inner().get_belief_to_var(edge_id))
    }
    //获取从给定变量到指定因子的信念。
    pub fn get_belief_from_var(&self, py: Python, var: &str, factor: &str) -> PyResult<PyObject> {
        let edge_id = self.get_edge_named(var, factor)?;
        distr2py(py, self.get_inner().get_belief_from_var(edge_id))
    }
    pub fn propagate_var(
        &mut self,
        py: Python,
        var: &str,
        config: crate::ConfigWrapper,
        clear_beliefs: bool,
    ) -> PyResult<()> {
        config.on_worker(py, |_| {
            let var_id = self.get_var(var)?;
            self.get_inner_mut().propagate_var(var_id, clear_beliefs);
            Ok(())
        })
    }
    pub fn propagate_all_vars(
        &mut self,
        py: Python,
        config: crate::ConfigWrapper,
        clear_beliefs: bool,
    ) -> PyResult<()> {
        config.on_worker(py, |_| {
            self.get_inner_mut().propagate_all_vars(clear_beliefs);
            Ok(())
        })
    }
    pub fn propagate_factor_all(
        &mut self,
        py: Python,
        factor: &str,
        config: crate::ConfigWrapper,
    ) -> PyResult<()> {
        config.on_worker(py, |_| {
            let factor_id = self.get_factor(factor)?;
            self.get_inner_mut().propagate_factor_all(factor_id);
            Ok(())
        })
    }
    pub fn set_belief_from_var(
        &mut self,
        py: Python,
        var: &str,
        factor: &str,
        distr: PyObject,
    ) -> PyResult<()> {
        let edge_id = self.get_edge_named(var, factor)?;
        let bp = self.get_inner_mut();
        let distr = obj2distr(py, distr, bp.get_graph().edge_multi(edge_id))?;
        bp.set_belief_from_var(edge_id, distr)
            .map_err(|e| PyTypeError::new_err(e.to_string()))?;
        Ok(())
    }
    pub fn set_belief_to_var(
        &mut self,
        py: Python,
        var: &str,
        factor: &str,
        distr: PyObject,
    ) -> PyResult<()> {
        let edge_id = self.get_edge_named(var, factor)?;
        let bp = self.get_inner_mut();
        let distr = obj2distr(py, distr, bp.get_graph().edge_multi(edge_id))?;
        bp.set_belief_to_var(edge_id, distr)
            .map_err(|e| PyTypeError::new_err(e.to_string()))?;
        Ok(())
    }

    pub fn propagate_factor(
        &mut self,
        py: Python,
        factor: &str,
        dest: Vec<&str>,
        clear_incoming: bool,
        config: crate::ConfigWrapper,
    ) -> PyResult<()> {
        config.on_worker(py, |_| {
            let factor_id = self.get_factor(factor)?;
            let dest = dest
                .iter()
                .map(|v| self.get_var(v))
                .collect::<Result<Vec<_>, _>>()?;
            self.get_inner_mut()
                .propagate_factor(factor_id, dest.as_slice(), clear_incoming);
            Ok(())
        })
    }
    //执行循环传播
    pub fn propagate_loopy_step(
        &mut self,
        py: Python,
        n_steps: u32,
        config: crate::ConfigWrapper,
        clear_beliefs: bool,
    ) {
        config.on_worker(py, |_| {
            self.get_inner_mut()
                .propagate_loopy_step(n_steps, clear_beliefs);
        });
    }
    pub fn graph(&self) -> FactorGraph {
        FactorGraph {
            inner: Some(self.get_inner().get_graph().clone()),
        }
    }
    //函数执行“无环”的传播
    pub fn propagate_acyclic(
        &mut self,
        py: Python,
        dest: &str,
        clear_intermediates: bool,
        clear_evidence: bool,
        config: crate::ConfigWrapper,
    ) -> PyResult<()> {
        config.on_worker(py, |_| {
            let var = self.get_var(dest)?;
            self.get_inner_mut()
                .propagate_acyclic(var, clear_intermediates, clear_evidence)
                .map_err(|e| PyValueError::new_err(e.to_string()))
        })
    }
}

fn obj2distr(py: Python, distr: PyObject, multi: bool) -> PyResult<sasca::Distribution> {
    if multi {
        let distr: &PyArray2<f64> = distr.extract(py)?;
        sasca::Distribution::from_array_multi(
            distr
                .readonly()
                .as_array()
                .as_standard_layout()
                .into_owned(),
        )
    } else {
        let distr: &PyArray1<f64> = distr.extract(py)?;
        sasca::Distribution::from_array_single(
            distr
                .readonly()
                .as_array()
                .as_standard_layout()
                .into_owned(),
        )
    }
    .map_err(|e| PyTypeError::new_err(e.to_string()))
}

fn obj2pub(py: Python, obj: PyObject, multi: bool) -> PyResult<sasca::PublicValue> {
    Ok(if multi {
        let obj: Vec<sasca::ClassVal> = obj.extract(py)?;
        sasca::PublicValue::Multi(obj)
    } else {
        let obj: sasca::ClassVal = obj.extract(py)?;
        sasca::PublicValue::Single(obj)
    })
}

fn distr2py(py: Python, distr: &sasca::Distribution) -> PyResult<PyObject> {
    if let Some(d) = distr.value() {
        if distr.multi() {
            return Ok(PyArray2::from_array(py, &d).into_py(py));
        } else {
            return Ok(PyArray1::from_array(py, &d.slice(ndarray::s![0, ..])).into_py(py));
        }
    } else {
        return Ok(py.None());
    }
}
